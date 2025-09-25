use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use codex_core::config::ConfigOverrides as CoreConfigOverrides;
use codex_core::protocol::AskForApproval;
use codex_protocol::config_types::SandboxMode as SandboxModeCfg;
use std::env;
use std::ffi::OsString;
use toml::Value as TomlValue;

#[derive(Parser, Debug)]
#[command(
    name = "codex-agentic",
    version,
    about = "Combined launcher: embedded CLI by default; ACP mode with 'acp'"
)]
struct CliArgs {
    #[command(subcommand)]
    cmd: Option<Cmd>,

    /// Legacy: run ACP server (deprecated; use `codex-agentic acp`)
    #[arg(long, hide = true)]
    acp: bool,

    /// Legacy: list models (OSS). Prefer `codex-agentic models list --oss`.
    #[arg(long, hide = true)]
    list_models: bool,

    /// Trailing args forwarded to embedded CLI when no subcommand is used
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    forward: Vec<OsString>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run ACP stdio server
    #[command(
        about = "Run ACP stdio server",
        long_about = "Runs the agent over stdin/stdout for editors (e.g., Zed).\n\nUse first-class flags for common settings (like --model, --oss). Use -c/--config for advanced or nested keys (key=value, JSON-parseable).",
        after_help = "Examples:\n  # Pick model + medium effort\n  codex-agentic acp --model gpt-4o-mini --model-reasoning-effort medium\n\n  # Use local Ollama provider + model\n  codex-agentic acp --oss -c model=\"qwq:latest\"\n\n  # Safer auto-exec in workspace\n  codex-agentic acp -c ask_for_approval=\"on-failure\" -c sandbox_mode=\"workspace-write\"\n\n  # Hide reasoning completely\n  codex-agentic acp -c model_reasoning_summary=\"none\" -c hide_agent_reasoning=true\n\n  # Set working directory\n  codex-agentic acp -c cwd=\"/path/to/project\"\n\n  # YOLO mode with search (dangerous): no approvals, no sandbox, enable web search\n  codex-agentic acp --yolo-with-search\n"
    )]
    Acp {
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long = "model-provider")]
        model_provider: Option<String>,
        #[arg(long = "model-reasoning-effort")]
        model_reasoning_effort: Option<String>,
        #[arg(long = "model-reasoning-summary")]
        model_reasoning_summary: Option<String>,
        #[arg(long = "model-verbosity")]
        model_verbosity: Option<String>,
        #[arg(long)]
        hide_agent_reasoning: bool,
        #[arg(long)]
        show_raw_agent_reasoning: bool,
        #[arg(long)]
        oss: bool,
        /// YOLO mode: no approvals, no sandbox, enable web search (dangerous!)
        #[arg(
            long = "yolo-with-search",
            help = "No approvals, no sandbox, and enable web search (DANGEROUS)"
        )]
        yolo_with_search: bool,
        #[arg(
            short = 'c',
            long = "config",
            help = "Override config: -c key=value (repeat). Values parse as JSON if possible."
        )]
        config_overrides: Vec<String>,
    },
    /// Launch the embedded CLI (default) and forward args to it
    Cli {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<OsString>,
    },
    /// Utilities
    #[command(subcommand)]
    Models(ModelsCmd),
    /// Print common examples and config recipes
    #[command(name = "help-recipes")]
    HelpRecipes,
    /// Resume a previous session (picker, last, or by id) while allowing normal flags
    #[command(
        about = "Resume a previous session",
        long_about = "Resume a recorded session. With no SESSION_ID and no --last, shows a picker.\n\nFlags like --yolo-with-search and --search are forwarded to the chat session.",
        after_help = "Examples:\n  codex-agentic resume\n  codex-agentic resume --last\n  codex-agentic resume <SESSION_ID>\n  codex-agentic resume --yolo --search\n  codex-agentic resume --last --yolo --search"
    )]
    Resume(ResumeArgs),
    /// Local codebase indexing (build/query/status/verify/clean)
    #[command(subcommand)]
    Index(IndexCmd),
    /// Semantic search in the local codebase (same engine as TUI `/search`)
    #[command(
        name = "search-code",
        about = "Search the local codebase (same as TUI /search)",
        long_about = "Runs a local semantic search over the repository index. Matches the behavior of the TUI /search command.",
        alias = "search"
    )]
    SearchCode(SearchCodeArgs),
}

#[derive(Subcommand, Debug)]
enum ModelsCmd {
    /// List models (currently supports --oss provider)
    List {
        #[arg(long)]
        oss: bool,
        #[arg(short = 'c', long = "config")]
        config_overrides: Vec<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum IndexCmd {
    /// Build or refresh the local index (default model: bge-small)
    Build(IndexBuildArgs),
    /// Query the local index for relevant code
    Query(IndexQueryArgs),
    /// Show index status
    Status,
    /// Verify index integrity
    Verify,
    /// Remove on-disk index
    Clean,
    /// Manage ignore patterns used by the indexer (stored in .index-ignore at repo root)
    Ignore(IndexIgnoreArgs),
}

#[derive(Args, Debug, Clone)]
struct IndexBuildArgs {
    /// Embedding model preset
    #[arg(long, value_parser = ["bge-small","bge-large"], default_value = "bge-small")]
    model: String,
    /// Force full rebuild instead of incremental
    #[arg(long)]
    force: bool,
    /// Chunking mode: auto (tree-sitter when available) | lines
    #[arg(long, value_parser = ["auto","lines"], default_value = "auto")]
    chunk: String,
    /// Target lines per chunk (lines mode)
    #[arg(long, default_value_t = 160)]
    lines: usize,
    /// Overlap lines between chunks (lines mode)
    #[arg(long, default_value_t = 32)]
    overlap: usize,
}

#[derive(Args, Debug, Clone)]
struct IndexQueryArgs {
    /// Free-text query
    query: String,
    /// Top-K results
    #[arg(short = 'k', long = "k", default_value_t = 8)]
    k: usize,
    /// Print snippet previews
    #[arg(long = "show-snippets")]
    show_snippets: bool,
    /// Output format: text | json | xml
    #[arg(long = "output", value_enum, default_value_t = OutputFormatArg::Text)]
    output: OutputFormatArg,
    /// Disable line numbers in snippets
    #[arg(long = "no-line-numbers", default_value_t = false)]
    no_line_numbers: bool,
    /// Line number column width
    #[arg(long = "line-number-width", default_value_t = 6)]
    line_number_width: usize,
    /// Show diff-style "+ " prefix for snippet lines
    #[arg(long = "diff", default_value_t = false)]
    diff: bool,
}

#[derive(Args, Debug, Clone)]
struct IndexIgnoreArgs {
    /// Add a pattern (glob-like: * and ? supported). Repeat to add multiple.
    #[arg(long = "add")]
    add: Vec<String>,
    /// Remove a pattern. Repeat to remove multiple.
    #[arg(long = "remove")]
    remove: Vec<String>,
    /// Reset to the default set and overwrite .index-ignore if it exists.
    #[arg(long = "reset")]
    reset: bool,
    /// List the current patterns and the file path.
    #[arg(long = "list")]
    list: bool,
}

#[derive(Args, Debug, Clone)]
struct SearchCodeArgs {
    /// Free-text query
    query: String,
    /// Top-K results
    #[arg(short = 'k', long = "k", default_value_t = 8)]
    k: usize,
    /// Print snippet previews
    #[arg(long = "show-snippets")]
    show_snippets: bool,
    /// Output format: text | json | xml
    #[arg(long = "output", value_enum, default_value_t = OutputFormatArg::Text)]
    output: OutputFormatArg,
    /// Disable line numbers in snippets
    #[arg(long = "no-line-numbers", default_value_t = false)]
    no_line_numbers: bool,
    /// Line number column width
    #[arg(long = "line-number-width", default_value_t = 6)]
    line_number_width: usize,
    /// Show diff-style "+ " prefix for snippet lines
    #[arg(long = "diff", default_value_t = false)]
    diff: bool,
}

#[derive(Clone, Debug, ValueEnum)]
enum OutputFormatArg {
    Text,
    Json,
    Xml,
}

#[derive(Args, Debug, Clone)]
struct ResumeArgs {
    /// Resume the most recent session
    #[arg(long = "last")]
    last: bool,
    /// Specific session id to resume
    #[arg(value_name = "SESSION_ID")]
    session_id: Option<String>,
    /// Additional flags forwarded to the embedded CLI (e.g., --yolo, --search)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    rest: Vec<OsString>,
}

fn main() -> Result<()> {
    let cli = CliArgs::parse();

    if let Some(cmd) = &cli.cmd {
        match cmd {
            Cmd::SearchCode(SearchCodeArgs {
                query,
                k,
                show_snippets,
                output,
                no_line_numbers,
                line_number_width,
                diff,
            }) => {
                let args = IndexQueryArgs {
                    query: query.to_string(),
                    k: *k,
                    show_snippets: *show_snippets,
                    output: output.clone(),
                    no_line_numbers: *no_line_numbers,
                    line_number_width: *line_number_width,
                    diff: *diff,
                };
                return indexing::dispatch(IndexCmd::Query(args));
            }

            Cmd::Resume(ResumeArgs {
                last,
                session_id,
                rest,
            }) => {
                // Build an argv that the embedded CLI understands, preserving flags order.
                let mut forwarded: Vec<OsString> = Vec::with_capacity(2 + rest.len());
                forwarded.push(OsString::from("resume"));
                if *last {
                    forwarded.push(OsString::from("--last"));
                }
                if let Some(id) = session_id.clone() {
                    forwarded.push(OsString::from(id));
                }
                forwarded.extend(rest.iter().cloned());
                return run_embedded_cli(&forwarded);
            }

            Cmd::Acp {
                model,
                profile,
                cwd,
                model_provider,
                model_reasoning_effort,
                model_reasoning_summary,
                model_verbosity,
                hide_agent_reasoning,
                show_raw_agent_reasoning,
                oss,
                yolo_with_search,
                config_overrides,
            } => {
                let mut overrides: Vec<(String, TomlValue)> = Vec::new();
                let mut typed_overrides: CoreConfigOverrides = CoreConfigOverrides::default();
                // Background indexers (non-blocking)
                indexing::spawn_first_run_if_enabled();
                indexing::spawn_periodic_maintenance();
                if *yolo_with_search {
                    // Add dangerous defaults first so specific -c keys can override them.
                    overrides.push(("ask_for_approval".into(), TomlValue::String("never".into())));
                    overrides.push((
                        "sandbox_mode".into(),
                        TomlValue::String("danger-full-access".into()),
                    ));
                    overrides.push(("tools.web_search_request".into(), TomlValue::Boolean(true)));

                    // Also set typed overrides for reliability across loaders.
                    typed_overrides.approval_policy = Some(AskForApproval::Never);
                    typed_overrides.sandbox_mode = Some(SandboxModeCfg::DangerFullAccess);
                    typed_overrides.tools_web_search_request = Some(true);
                }
                if let Some(v) = model {
                    overrides.push(("model".into(), TomlValue::String(v.clone())));
                }
                if let Some(v) = profile {
                    overrides.push(("profile".into(), TomlValue::String(v.clone())));
                }
                if let Some(v) = cwd {
                    overrides.push(("cwd".into(), TomlValue::String(v.clone())));
                }
                if let Some(v) = model_provider {
                    overrides.push(("model_provider".into(), TomlValue::String(v.clone())));
                }
                if let Some(v) = model_reasoning_effort {
                    overrides.push((
                        "model_reasoning_effort".into(),
                        TomlValue::String(v.clone()),
                    ));
                }
                if let Some(v) = model_reasoning_summary {
                    overrides.push((
                        "model_reasoning_summary".into(),
                        TomlValue::String(v.clone()),
                    ));
                }
                if let Some(v) = model_verbosity {
                    overrides.push(("model_verbosity".into(), TomlValue::String(v.clone())));
                }
                if *hide_agent_reasoning {
                    overrides.push(("hide_agent_reasoning".into(), TomlValue::Boolean(true)));
                }
                if *show_raw_agent_reasoning {
                    overrides.push(("show_raw_agent_reasoning".into(), TomlValue::Boolean(true)));
                }
                if *oss {
                    typed_overrides.model_provider =
                        Some(codex_core::BUILT_IN_OSS_MODEL_PROVIDER_ID.to_string());
                }
                for kv in config_overrides {
                    if let Some(eq) = kv.find('=') {
                        let (k, vraw) = kv.split_at(eq);
                        let vraw = &vraw[1..];
                        let val = toml::from_str::<TomlValue>(vraw)
                            .unwrap_or_else(|_| TomlValue::String(vraw.to_string()));
                        overrides.push((k.to_string(), val));
                    }
                }
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .enable_io()
                    .build()
                    .context("tokio runtime for ACP stdio")?;
                return rt.block_on(async move {
                    codex_acp::run_stdio_with_overrides(overrides, typed_overrides).await
                });
            }
            Cmd::Models(ModelsCmd::List {
                oss,
                config_overrides,
            }) => {
                let mut kv_overrides: Vec<(String, TomlValue)> = Vec::new();
                if *oss {
                    kv_overrides.push(("model_provider".into(), TomlValue::String("oss".into())));
                }
                for kv in config_overrides {
                    if let Some(eq) = kv.find('=') {
                        let (k, vraw) = kv.split_at(eq);
                        let vraw = &vraw[1..];
                        let val = toml::from_str::<TomlValue>(vraw)
                            .unwrap_or_else(|_| TomlValue::String(vraw.to_string()));
                        kv_overrides.push((k.to_string(), val));
                    }
                }
                list_models(kv_overrides)?;
                return Ok(());
            }
            Cmd::Cli { args } => {
                return run_embedded_cli(args);
            }
            Cmd::HelpRecipes => {
                print_recipes();
                return Ok(());
            }
            Cmd::Index(index_cmd) => {
                return indexing::dispatch(index_cmd.clone());
            }
        }
    }

    // Legacy flags fallback
    if cli.acp {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()
            .context("tokio runtime for ACP stdio")?;
        return rt.block_on(async move {
            codex_acp::run_stdio_with_overrides(Vec::new(), CoreConfigOverrides::default()).await
        });
    }
    if cli.list_models {
        list_models(vec![(
            "model_provider".into(),
            TomlValue::String("oss".into()),
        )])?;
        return Ok(());
    }

    // Default: embedded CLI
    run_embedded_cli(&cli.forward)
}

fn list_models(kv_overrides: Vec<(String, TomlValue)>) -> Result<()> {
    let cfg = codex_core::config::Config::load_with_cli_overrides(
        kv_overrides,
        CoreConfigOverrides::default(),
    )
    .context("load config for models list")?;
    if cfg.model_provider_id != "oss" {
        eprintln!("models list currently supports --oss (Ollama) provider only.");
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()
        .context("tokio runtime for models list")?;
    rt.block_on(async move {
        match codex_ollama::OllamaClient::try_from_oss_provider(&cfg).await {
            Ok(client) => match client.fetch_models().await {
                Ok(models) => {
                    for m in models {
                        println!("{}", m);
                    }
                }
                Err(e) => eprintln!("error: failed to fetch models: {}", e),
            },
            Err(e) => eprintln!("error: {}", e),
        }
    });
    Ok(())
}

fn run_embedded_cli(args: &[OsString]) -> Result<()> {
    // Configure upgrade banner to point to our repo/README
    unsafe {
        std::env::set_var("CODEX_CURRENT_VERSION", env!("CARGO_PKG_VERSION"));
    }
    let repo = std::env::var("CODEX_AGENTIC_UPDATE_REPO")
        .ok()
        .or_else(|| {
            let url = env!("CARGO_PKG_REPOSITORY");
            extract_repo_slug(url)
        })
        .or_else(|| std::env::var("GITHUB_REPOSITORY").ok());
    if std::env::var("CODEX_UPDATE_LATEST_URL").is_err() {
        if let Some(r) = &repo {
            unsafe {
                std::env::set_var(
                    "CODEX_UPDATE_LATEST_URL",
                    format!("https://api.github.com/repos/{r}/releases/latest"),
                );
            }
        } else {
            unsafe {
                std::env::set_var("CODEX_DISABLE_UPDATE_CHECK", "1");
            }
        }
    }
    if std::env::var("CODEX_UPGRADE_URL").is_err()
        && let Some(r) = &repo
    {
        unsafe {
            std::env::set_var("CODEX_UPGRADE_URL", format!("https://github.com/{r}"));
        }
    }
    if let Ok(cmd) = std::env::var("CODEX_AGENTIC_UPGRADE_CMD") {
        unsafe {
            std::env::set_var("CODEX_UPGRADE_CMD", cmd);
        }
    }

    // Handle the resume command specially
    let cli = if args.first().and_then(|s| s.to_str()) == Some("resume") {
        // Parse without the "resume" argument first
        let remaining_args = if args.len() > 1 { &args[1..] } else { &[] };
        let mut cli = codex_tui::Cli::parse_from(
            std::iter::once(OsString::from("codex-agentic")).chain(remaining_args.iter().cloned()),
        );

        // Determine resume options from remaining args, allowing additional flags like
        // --yolo / --search without misinterpreting them as a session id.
        if remaining_args.is_empty() {
            // No arguments after resume means picker
            cli.resume_picker = true;
        } else if remaining_args
            .iter()
            .any(|s| s.to_str().is_some_and(|t| t == "--last"))
        {
            cli.resume_last = true;
        } else {
            // Find the first non-flag token; treat it as a session id if present.
            let id_opt = remaining_args
                .iter()
                .filter_map(|s| s.to_str())
                .find(|t| !t.starts_with('-'))
                .map(String::from);
            if id_opt.is_some() {
                cli.resume_session_id = id_opt;
            } else {
                // Only flags after 'resume' → show picker
                cli.resume_picker = true;
            }
        }
        cli
    } else {
        codex_tui::Cli::parse_from(
            std::iter::once(OsString::from("codex-agentic")).chain(args.iter().cloned()),
        )
    };

    // Background indexers (non-blocking)
    indexing::spawn_first_run_if_enabled();
    indexing::spawn_periodic_maintenance();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()?;
    rt.block_on(async move {
        let _ = codex_tui::run_main(cli, None).await;
        Ok(())
    })
}

fn extract_repo_slug(url: &str) -> Option<String> {
    let prefix = "https://github.com/";
    if let Some(path) = url.strip_prefix(prefix) {
        let slug = path
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .split('/')
            .take(2)
            .collect::<Vec<_>>()
            .join("/");
        if slug.split('/').count() == 2 {
            return Some(slug);
        }
    }
    None
}

fn print_recipes() {
    const RECIPES: &str = r#"
Quick Recipes
=============

1) Pick a model + medium effort
   codex-agentic acp --model gpt-4o-mini --model-reasoning-effort medium

2) Use local Ollama provider + model
   codex-agentic acp --oss -c model="qwq:latest"

3) Safer auto-exec in workspace (no prompts on success)
   codex-agentic acp -c ask_for_approval="on-failure" -c sandbox_mode="workspace-write"

4) Hide reasoning completely
   codex-agentic acp -c model_reasoning_summary="none" -c hide_agent_reasoning=true

5) Set working directory
   codex-agentic acp -c cwd="/path/to/project"

6) YOLO mode with search (DANGEROUS: no approvals, no sandbox)
   codex-agentic acp --yolo-with-search

7) Resume a previous session (picker)
   codex-agentic resume

8) Resume most recent session with flags
   codex-agentic resume --last --yolo --search

9) Resume a specific session by id
   codex-agentic resume <SESSION_ID> --search

10) See full upstream CLI commands
   codex-agentic cli -- --help

Hint
 -c/--config accepts key=value (JSON parsed when possible). Repeat -c to set multiple values.
 Prefer first-class flags when available: --model, --oss, --profile, --cwd, --model-reasoning-effort, etc.
"#;
    println!("{}", RECIPES);
}

mod indexing;
