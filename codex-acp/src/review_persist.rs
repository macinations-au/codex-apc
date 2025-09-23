use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitSnapshot {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_dirty: bool,
    pub commit_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportBody {
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonReport {
    pub schema: String,
    pub generated_at: DateTime<Utc>,
    pub workspace_root: PathBuf,
    pub git: GitSnapshot,
    pub context_source: String,
    pub model: Option<String>,
    pub token_usage: Option<TokenUsage>,
    pub inputs_hash: String,
    pub inputs: Vec<PathBuf>,
    pub references: Vec<PathBuf>,
    pub report: ReportBody,
}

const REPORT_SCHEMA: &str = "codex-agentic/review-codebase@v1";

pub fn load_previous_report_sync(cwd: &Path) -> anyhow::Result<JsonReport> {
    let path = cwd.join(".codex/review-codebase.json");
    let data = fs::read(&path)?;
    let report: JsonReport = serde_json::from_slice(&data)?;
    Ok(report)
}

pub fn update_report_markdown_sync(
    cwd: &Path,
    markdown: &str,
    model: Option<String>,
) -> anyhow::Result<()> {
    // Load or initialize
    let mut report = load_previous_report_sync(cwd).unwrap_or_else(|_| JsonReport {
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
    report.generated_at = Utc::now();
    report.model = model;
    report.report.markdown = sanitize_markdown_headings(markdown);

    let dir = cwd.join(".codex");
    fs::create_dir_all(&dir)?;
    let final_path = dir.join("review-codebase.json");
    let tmp_path = dir.join("review-codebase.json.tmp");
    let data = serde_json::to_vec_pretty(&report)?;
    fs::write(&tmp_path, &data)?;
    fs::rename(&tmp_path, &final_path)?;
    Ok(())
}

/// Ensure headings like "## Title" begin on a new line (outside code fences).
fn sanitize_markdown_headings(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
    let mut in_fence = false;
    let mut at_line_start = true;
    let mut i = 0;
    let bytes = input.as_bytes();
    while i < bytes.len() {
        if at_line_start && bytes[i..].starts_with(b"```") {
            in_fence = !in_fence;
            out.push_str("```");
            i += 3;
            at_line_start = false;
            continue;
        }
        if !in_fence && !at_line_start && bytes[i] == b'#' {
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
                    for _ in 0..hashes {
                        out.push('#');
                    }
                    out.push(' ');
                    i = j + 1;
                    at_line_start = false;
                    continue;
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

/// Public wrapper to sanitize saved markdown before sending to clients.
pub fn sanitize_markdown_for_display(input: &str) -> String {
    sanitize_markdown_headings(input)
}

/// Return true if `next` starts with an ATX heading like `## `.
fn next_starts_with_atx_heading(next: &str) -> bool {
    let bytes = next.as_bytes();
    let mut i = 0;
    let mut hashes = 0;
    while i < bytes.len() && bytes[i] == b'#' && hashes < 6 {
        hashes += 1;
        i += 1;
    }
    hashes > 0 && i < bytes.len() && bytes[i] == b' '
}

/// Scan `prev` and return whether we end inside a fenced code block.
fn in_code_fence_at_end(prev: &str) -> bool {
    let bytes = prev.as_bytes();
    let mut in_fence = false;
    let mut at_line_start = true;
    let mut i = 0;
    while i < bytes.len() {
        if at_line_start && bytes[i..].starts_with(b"```") {
            in_fence = !in_fence;
            i += 3;
            at_line_start = false;
            continue;
        }
        let ch = match prev[i..].chars().next() {
            Some(c) => c,
            None => break,
        };
        i += ch.len_utf8();
        if ch == '\n' {
            at_line_start = true;
        } else if at_line_start {
            at_line_start = false;
        }
    }
    in_fence
}

/// Determine if we should insert a blank line before `next` to ensure a heading
/// begins on a new line when streaming.
pub fn needs_newline_before_heading(prev: &str, next: &str) -> bool {
    if prev.is_empty() || prev.ends_with('\n') {
        return false;
    }
    if in_code_fence_at_end(prev) {
        return false;
    }
    next_starts_with_atx_heading(next)
}
