use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::{DateTime, Utc};
use codex_core::config::Config;
use codex_core::protocol::{InputItem, Op};
use sha2::{Digest, Sha256};
use tokio::fs as tokio_fs;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{Duration, interval};

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::history_cell;
use crate::history_cell::AgentMessageCell;
use crate::markdown::append_markdown;
use ratatui::text::Line;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub modified_at: Option<DateTime<Utc>>,
    pub sha256: String,
    pub binary: bool,
    pub sampled_text: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct GitSnapshot {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_dirty: bool,
    pub commit_hash: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReviewInputs {
    pub commit_hash: Option<String>,
    pub context_source: &'static str, // "git" | "filesystem"
    pub files: Vec<FileEntry>,        // curated + changed
    pub inputs_hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct JsonReport {
    pub schema: String,
    pub generated_at: DateTime<Utc>,
    pub workspace_root: PathBuf,
    pub git: GitSnapshot,
    pub context_source: String,
    pub model: Option<String>,
    pub token_usage: Option<TokenUsage>,
    pub inputs_hash: String,
    pub inputs: Vec<FileEntry>,
    pub references: Vec<PathBuf>,
    pub report: ReportBody,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ReportBody {
    pub markdown: String,
}

const REPORT_SCHEMA: &str = "codex-agentic/review-codebase@v1";

/// Top-level entry invoked by the TUI when `/about-codebase` is run.
pub async fn run_review_codebase(
    app_tx: AppEventSender,
    config: Config,
    _previous_report: Option<JsonReport>,
    force: bool,
    // NOTE: we intentionally load the previous report from disk to avoid
    // crossing more state through the widget.
) -> anyhow::Result<()> {
    let cwd = config.cwd.clone();

    // Defer scanning status until we actually need to scan.

    let (inside_git, git) = get_git_snapshot(&cwd)
        .await
        .unwrap_or((false, GitSnapshot::default()));

    // Load previous report
    let prev = load_previous_report(&cwd).ok();

    if !force {
        if let Some(prev) = &prev {
            // Always show the previous report first for quick context.
            let ts = prev.generated_at.to_rfc3339();
            app_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_info_event(format!("Showing last codebase report ({ts})"), None),
            )));
            if prev.report.markdown.trim().is_empty() {
                app_tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_info_event(
                        "No saved report content yet — run /about-codebase --refresh to generate one.".to_string(),
                        None,
                    ),
                )));
            } else {
                // Ensure headings start on a new line so markdown renders correctly.
                let sanitized = sanitize_markdown_headings(&prev.report.markdown);
                let mut rendered: Vec<Line<'static>> = Vec::new();
                append_markdown(&sanitized, &mut rendered, &config);
                let cell = AgentMessageCell::new(rendered, true);
                app_tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
            }

            // Decide whether to suggest an update (stale or changes found)
            let age = Utc::now() - prev.generated_at;
            let is_stale = age.num_hours() >= 24;
            let changed = if inside_git {
                prev.git.commit_hash.as_deref() != git.commit_hash.as_deref() || git.is_dirty
            } else {
                false
            };
            if is_stale || changed {
                let mut hint = String::new();
                if is_stale {
                    hint.push_str(&format!("Report is {}h old. ", age.num_hours()));
                }
                if changed {
                    hint.push_str("Changes detected since last review. ");
                }
                hint.push_str("Run /about-codebase --refresh to regenerate now.");
                app_tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_info_event("Update available".to_string(), Some(hint)),
                )));
            }

            // Ask ChatWidget to memorize once per session (no-op if already done).
            if !prev.report.markdown.trim().is_empty() {
                let sanitized = sanitize_markdown_headings(&prev.report.markdown);
                app_tx.send(AppEvent::MemorizeReportIfNeeded(sanitized));
            }
            return Ok(());
        }
        // No previous report; inform the user this initial run may take time (cyan status style).
        app_tx.send(AppEvent::InsertHistoryCell(Box::new(
            history_cell::new_review_status_line(
                "First time running code check — generating the report…".to_string(),
            ),
        )));
        app_tx.send(AppEvent::InsertHistoryCell(Box::new(
            history_cell::new_review_status_line(
                "Please wait, this may take some time :)".to_string(),
            ),
        )));
    }

    // Fast-path: if in Git with clean worktree and same HEAD as last run, just render previous report.
    if inside_git
        && !git.is_dirty
        && prev
            .as_ref()
            .and_then(|p| p.git.commit_hash.clone())
            .as_deref()
            == git.commit_hash.as_deref()
        && let Some(prev) = prev
    {
        let ts = prev.generated_at.to_rfc3339();
        app_tx.send(AppEvent::InsertHistoryCell(Box::new(
            history_cell::new_info_event(
                format!("No changes since last review — showing previous report ({ts})"),
                None,
            ),
        )));
        if prev.report.markdown.trim().is_empty() {
            app_tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_info_event(
                    "No saved report content yet — run /about-codebase --refresh to generate one."
                        .to_string(),
                    None,
                ),
            )));
        } else {
            // Ensure headings start on a new line so markdown renders correctly.
            let sanitized = sanitize_markdown_headings(&prev.report.markdown);
            let mut rendered: Vec<Line<'static>> = Vec::new();
            append_markdown(&sanitized, &mut rendered, &config);
            let cell = AgentMessageCell::new(rendered, true);
            app_tx.send(AppEvent::InsertHistoryCell(Box::new(cell)));
            // Ask ChatWidget to memorize once per session (no-op if already done).
            let sanitized = sanitize_markdown_headings(&prev.report.markdown);
            app_tx.send(AppEvent::MemorizeReportIfNeeded(sanitized));
        }
        return Ok(());
    }

    // Determine file set to include (curated + deltas)
    let candidates = if inside_git {
        curate_initial_file_set(&cwd)
            .union(&git_delta_changed_paths(prev.as_ref(), &git).await)
            .cloned()
            .collect::<BTreeSet<PathBuf>>()
    } else {
        curate_initial_file_set(&cwd)
    };

    app_tx.send(AppEvent::InsertHistoryCell(Box::new(
        history_cell::new_review_status_line(format!(
            "Reading {} files (rate-limited)…",
            candidates.len()
        )),
    )));

    let mut files: Vec<FileEntry> = Vec::new();
    let mut ticker = interval(Duration::from_millis(100)); // ~10 files/sec
    for path in candidates {
        ticker.tick().await;
        if let Ok(entry) = read_and_sample(&cwd, &path).await {
            files.push(entry);
        }
    }

    // Compute inputs hash for persistence; since this is force or no-previous, proceed to build prompt.
    let _inputs_hash = compute_inputs_hash(git.commit_hash.as_deref(), &files);

    // Build prompt
    app_tx.send(AppEvent::InsertHistoryCell(Box::new(
        history_cell::new_review_status_line("Composing prompt…".to_string()),
    )));
    let prompt = assemble_prompt(&cwd, &git, prev.as_ref(), &files, 200 * 1024);

    // Submit to model via standard user input
    app_tx.send(AppEvent::CodexOp(Op::UserInput {
        items: vec![InputItem::Text {
            text: prompt.clone(),
        }],
    }));

    // Do not overwrite existing report until final markdown is available.
    // The final save occurs after generation completes (see ChatWidget::on_task_complete).

    Ok(())
}

fn compute_inputs_hash(commit: Option<&str>, files: &[FileEntry]) -> String {
    let mut hasher = Sha256::new();
    if let Some(c) = commit {
        hasher.update(c.as_bytes());
    }
    // stable order by path
    let mut rows: Vec<(String, &FileEntry)> = files
        .iter()
        .map(|f| (f.path.to_string_lossy().to_string(), f))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (p, f) in rows {
        hasher.update(p.as_bytes());
        hasher.update([0]);
        hasher.update(f.sha256.as_bytes());
        hasher.update([0]);
        hasher.update(f.size_bytes.to_le_bytes());
    }
    hex::encode(hasher.finalize())
}

fn curate_initial_file_set(cwd: &Path) -> BTreeSet<PathBuf> {
    let mut set: BTreeSet<PathBuf> = BTreeSet::new();
    let push_if = |set: &mut BTreeSet<PathBuf>, p: &Path| {
        if p.starts_with(cwd) && p.exists() {
            set.insert(p.strip_prefix(cwd).unwrap().to_path_buf());
        }
    };
    // High-signal files
    push_if(&mut set, &cwd.join("README.md"));
    push_if(&mut set, &cwd.join("AGENTS.md"));
    push_if(
        &mut set,
        &cwd.join("docs/todo-review-codebase-implementation.md"),
    );
    // Cargo manifests for local crates
    for dir in ["codex-agentic", "codex-tui", "codex-acp", "."] {
        let p = cwd.join(dir).join("Cargo.toml");
        push_if(&mut set, &p);
    }
    // Key TUI entry points
    for p in [
        cwd.join("codex-tui/src/slash_command.rs"),
        cwd.join("codex-tui/src/chatwidget.rs"),
        cwd.join("codex-tui/src/get_git_diff.rs"),
        cwd.join("codex-tui/src/lib.rs"),
    ] {
        push_if(&mut set, &p);
    }
    // Some workflows if present
    for dirent in cwd
        .join(".github/workflows")
        .read_dir()
        .into_iter()
        .flatten()
        .flatten()
    {
        set.insert(PathBuf::from(".github/workflows").join(dirent.file_name()));
    }
    set
}

async fn git_delta_changed_paths(
    prev: Option<&JsonReport>,
    git: &GitSnapshot,
) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let Some(prev_commit) = prev.and_then(|p| p.git.commit_hash.clone()) else {
        return out;
    };
    let Some(cur_commit) = git.commit_hash.clone() else {
        return out;
    };
    if prev_commit == cur_commit {
        return out;
    }

    // git diff --name-status <prev>..HEAD
    let args = [
        "diff",
        "--name-status",
        &format!("{}..{}", prev_commit, cur_commit),
    ];
    let output = Command::new("git").args(args).output().await.ok();
    let Some(output) = output else {
        return out;
    };
    if !(output.status.success() || output.status.code() == Some(1)) {
        return out;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        // Formats: "A\tpath" | "M\tpath" | "D\tpath"
        let mut parts = line.split_whitespace();
        let _status = parts.next();
        if let Some(path) = parts.next() {
            out.insert(PathBuf::from(path));
        }
    }
    out
}

async fn get_git_snapshot(cwd: &Path) -> io::Result<(bool, GitSnapshot)> {
    let mut snap = GitSnapshot::default();
    // Check if inside a repo
    let status = Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .current_dir(cwd)
        .status()
        .await?;
    if !status.success() {
        return Ok((false, snap));
    }
    // branch
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(cwd)
        .output()
        .await?;
    if branch.status.success() {
        snap.branch = Some(String::from_utf8_lossy(&branch.stdout).trim().to_string());
    }
    // head commit
    let head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .await?;
    if head.status.success() {
        let h = String::from_utf8_lossy(&head.stdout).trim().to_string();
        snap.head = Some(h.clone());
        snap.commit_hash = Some(h);
    }
    // dirty (any status output)
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(cwd)
        .output()
        .await?;
    snap.is_dirty = !dirty.stdout.is_empty();

    Ok((true, snap))
}

async fn read_and_sample(cwd: &Path, rel_path: &Path) -> anyhow::Result<FileEntry> {
    let full_path = cwd.join(rel_path);
    let meta = tokio_fs::metadata(&full_path).await?;
    let size = meta.len();
    let modified_at = meta.modified().ok().map(DateTime::<Utc>::from);
    let file = tokio_fs::File::open(&full_path).await?;
    let mut buf = Vec::new();
    let max_preview: usize = 64 * 1024; // 64 KiB per file
    let read_len = std::cmp::min(size as usize, max_preview);
    let mut take = file.take(read_len as u64);
    take.read_to_end(&mut buf).await?;

    // hash full file (streamed)
    let sha256 = hash_file_sha256(&full_path)
        .await
        .unwrap_or_else(|_| "".into());
    let (binary, sampled_text) = sample_text(&buf, size as usize);

    Ok(FileEntry {
        path: rel_path.to_path_buf(),
        size_bytes: size,
        modified_at,
        sha256,
        binary,
        sampled_text,
    })
}

fn sample_text(buf: &[u8], total_size: usize) -> (bool, Option<String>) {
    // binary heuristic: contains NUL or invalid UTF-8
    let looks_binary = buf.contains(&0);
    match std::str::from_utf8(buf) {
        Ok(s) if !looks_binary => {
            let snippet = if s.len() > 8 * 1024 {
                let head = &s[..std::cmp::min(3 * 1024, s.len())];
                let mid_start = s.len() / 2;
                let mid = &s[mid_start..std::cmp::min(mid_start + 2 * 1024, s.len())];
                let tail = &s[s.len().saturating_sub(3 * 1024)..];
                format!(
                    "{head}\n\n--- sampled {} of {} bytes ---\n\n{mid}\n\n{tail}",
                    buf.len(),
                    total_size
                )
            } else {
                s.to_string()
            };
            (false, Some(snippet))
        }
        _ => (true, None),
    }
}

async fn hash_file_sha256(path: &Path) -> io::Result<String> {
    // Stream file to avoid loading huge files entirely
    let mut file = tokio_fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn assemble_prompt(
    cwd: &Path,
    git: &GitSnapshot,
    previous: Option<&JsonReport>,
    files: &[FileEntry],
    mut budget: usize,
) -> String {
    let mut out = String::new();
    fn push_line(buf: &mut String, line: &str, budget: &mut usize) {
        if *budget == 0 {
            return;
        }
        let needed = line.len() + 1;
        if needed > *budget {
            return;
        }
        buf.push_str(line);
        buf.push('\n');
        *budget -= needed;
    }

    push_line(&mut out, "# /about-codebase", &mut budget);
    push_line(
        &mut out,
        "Please produce a concise, high-signal codebase review.",
        &mut budget,
    );
    push_line(
        &mut out,
        "Focus on architecture, flows, CI/Release, config/env, design choices, and risks.",
        &mut budget,
    );
    push_line(&mut out, "", &mut budget);

    // context
    let git_line = match (
        git.branch.as_deref(),
        git.commit_hash.as_deref(),
        git.is_dirty,
    ) {
        (Some(b), Some(h), d) => format!("Git: {b}@{h}{}", if d { " (dirty)" } else { "" }),
        _ => "Git: (not a repository)".to_string(),
    };
    push_line(
        &mut out,
        &format!("Workspace: {}", cwd.display()),
        &mut budget,
    );
    push_line(&mut out, &git_line, &mut budget);
    push_line(&mut out, "", &mut budget);

    if let Some(prev) = previous {
        push_line(&mut out, "## Previous Review", &mut budget);
        for line in prev.report.markdown.lines() {
            push_line(&mut out, line, &mut budget);
        }
        push_line(&mut out, "", &mut budget);
    }

    push_line(&mut out, "## Embedded Contents", &mut budget);
    // Sort files by path for deterministic order
    let mut by_path: Vec<&FileEntry> = files.iter().collect();
    by_path.sort_by(|a, b| a.path.cmp(&b.path));
    for f in by_path {
        if budget < 2048 {
            break;
        }
        push_line(&mut out, &format!("### {}", f.path.display()), &mut budget);
        if let Some(text) = &f.sampled_text {
            push_line(&mut out, "```", &mut budget);
            for line in text.lines() {
                push_line(&mut out, line, &mut budget);
            }
            push_line(&mut out, "```", &mut budget);
        } else {
            push_line(&mut out, "(binary or too large; omitted)", &mut budget);
        }
        push_line(&mut out, "", &mut budget);
    }

    push_line(&mut out, "## Output Format", &mut budget);
    push_line(
        &mut out,
        "Return a Markdown report with sections: Architecture, Important Flows, CI/Release, Config & Env, Design Choices, Risks.",
        &mut budget,
    );
    out
}

pub fn load_previous_report(cwd: &Path) -> anyhow::Result<JsonReport> {
    let path = cwd.join(".codex/review-codebase.json");
    let data = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let report: JsonReport =
        serde_json::from_slice(&data).with_context(|| format!("parse JSON {}", path.display()))?;
    Ok(report)
}

pub async fn save_report_atomic(cwd: &Path, report: &JsonReport) -> anyhow::Result<()> {
    let dir = cwd.join(".codex");
    tokio_fs::create_dir_all(&dir).await.ok();
    let final_path = dir.join("review-codebase.json");
    let tmp_path = dir.join("review-codebase.json.tmp");
    let data = serde_json::to_vec_pretty(report)?;
    tokio_fs::write(&tmp_path, &data).await?;
    tokio_fs::rename(&tmp_path, &final_path).await?;
    Ok(())
}

/// Update the saved report's markdown and basic metadata (time/model/token usage),
/// preserving existing inputs metadata when present.
pub async fn update_report_markdown(
    cwd: &Path,
    markdown: &str,
    model: Option<String>,
    _token_usage_total: Option<u32>,
) -> anyhow::Result<()> {
    let mut report = load_previous_report(cwd).unwrap_or_else(|_| JsonReport {
        schema: REPORT_SCHEMA.to_string(),
        generated_at: Utc::now(),
        workspace_root: cwd.to_path_buf(),
        git: GitSnapshot::default(),
        context_source: "filesystem".to_string(),
        model: None,
        token_usage: None,
        inputs_hash: String::new(),
        inputs: Vec::new(),
        references: Vec::new(),
        report: ReportBody {
            markdown: String::new(),
        },
    });

    // Refresh git snapshot opportunistically
    if let Ok((inside_git, snap)) = get_git_snapshot(cwd).await {
        report.git = snap;
        report.context_source = if inside_git { "git" } else { "filesystem" }.to_string();
    }

    report.generated_at = Utc::now();
    if let Some(m) = model {
        report.model = Some(m);
    }
    // Sanitize before saving so future renders are clean across UIs.
    report.report.markdown = sanitize_markdown_headings(markdown);
    save_report_atomic(cwd, &report).await
}

/// Ensure that ATX headings ("#", "##", …) begin at the start of a line.
/// This fixes cases where streaming concatenation produces "…sentence.## Heading".
/// Skips transformations inside fenced code blocks.
pub(crate) fn sanitize_markdown_headings(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    let mut in_fence = false;
    let mut at_line_start = true;
    let mut i = 0;
    let bytes = input.as_bytes();
    while i < bytes.len() {
        // Detect fenced code block start/end at line start: "```"
        if at_line_start && bytes[i..].starts_with(b"```") {
            in_fence = !in_fence;
            out.push_str("```");
            i += 3;
            at_line_start = false;
            continue;
        }
        if !in_fence && !at_line_start {
            // If we see a heading marker not at line start, inject a blank line before it
            // and copy the entire heading marker sequence in one go.
            if bytes[i] == b'#' {
                let mut j = i;
                let mut hashes = 0;
                while j < bytes.len() && bytes[j] == b'#' && hashes < 6 {
                    hashes += 1;
                    j += 1;
                }
                if hashes > 0 && j < bytes.len() && bytes[j] == b' ' {
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                    // Copy the full heading prefix (### + space)
                    for _ in 0..hashes {
                        out.push('#');
                    }
                    out.push(' ');
                    i = j + 1; // skip the space as well
                    at_line_start = false;
                    continue;
                }
            }
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
        if ch == '\n' {
            at_line_start = true;
        } else if at_line_start {
            at_line_start = false;
        }
    }
    out
}

// ------------------------
// Tests (unit)
// ------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inputs_hash_deterministic() {
        let files = vec![
            FileEntry {
                path: PathBuf::from("a.txt"),
                size_bytes: 10,
                modified_at: None,
                sha256: "aa".into(),
                binary: false,
                sampled_text: Some("foo".into()),
            },
            FileEntry {
                path: PathBuf::from("b.txt"),
                size_bytes: 20,
                modified_at: None,
                sha256: "bb".into(),
                binary: false,
                sampled_text: Some("bar".into()),
            },
        ];
        let h1 = compute_inputs_hash(Some("deadbeef"), &files);
        let h2 = compute_inputs_hash(Some("deadbeef"), &files);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_sample_text_binary_detection() {
        let (bin, txt) = sample_text(b"\x00\x01\x02", 3);
        assert!(bin);
        assert!(txt.is_none());
        let (bin, txt) = sample_text(b"hello world", 11);
        assert!(!bin);
        assert_eq!(txt.as_deref(), Some("hello world"));
    }

    #[test]
    fn test_assemble_prompt_budget() {
        let files = vec![FileEntry {
            path: PathBuf::from("large.txt"),
            size_bytes: 1_000_000,
            modified_at: None,
            sha256: "ff".into(),
            binary: false,
            sampled_text: Some("x".repeat(50_000)),
        }];
        let prompt = assemble_prompt(
            Path::new("/tmp"),
            &GitSnapshot::default(),
            None,
            &files,
            10_000,
        );
        assert!(prompt.len() <= 10_000);
    }

    #[test]
    fn test_sanitize_headings_inserts_newline() {
        let input = "I'll scan…## Architecture\nDetails";
        let out = sanitize_markdown_headings(input);
        eprintln!("OUT=<{out}>");
        assert!(out.contains("\n\n## Architecture"));
    }
}
