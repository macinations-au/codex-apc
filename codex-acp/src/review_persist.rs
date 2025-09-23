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
    report.report.markdown = markdown.to_string();

    let dir = cwd.join(".codex");
    fs::create_dir_all(&dir)?;
    let final_path = dir.join("review-codebase.json");
    let tmp_path = dir.join("review-codebase.json.tmp");
    let data = serde_json::to_vec_pretty(&report)?;
    fs::write(&tmp_path, &data)?;
    fs::rename(&tmp_path, &final_path)?;
    Ok(())
}
