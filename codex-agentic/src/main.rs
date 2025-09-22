use anyhow::{Context, Result};
use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use toml::map::Map as TomlMap;
use toml::Value as TomlValue;
use clap::Parser as _;

fn main() -> Result<()> {
    // Early fast-path: support `--list-models` in CLI mode before any passthrough.
    {
        let args_utf: Vec<String> = env::args().skip(1).collect();
        if args_utf.iter().any(|a| a == "--list-models") {
            // Build minimal overrides: honor --oss/--OSS and -c/--config pairs
            let mut kv_overrides: Vec<(String, TomlValue)> = Vec::new();
            if args_utf.iter().any(|a| a == "--oss" || a == "--OSS") {
                kv_overrides.push(("model_provider".to_string(), TomlValue::String("oss".into())));
            }
            let mut i = 0;
            while i < args_utf.len() {
                let a = &args_utf[i];
                if a == "-c" || a == "--config" {
                    if i + 1 < args_utf.len() {
                        let s = &args_utf[i + 1];
                        if let Some(eq) = s.find('=') {
                            let (k, vraw) = s.split_at(eq);
                            let vraw = &vraw[1..];
                            let val = toml::from_str::<TomlValue>(vraw).unwrap_or_else(|_| TomlValue::String(vraw.to_string()));
                            kv_overrides.push((k.to_string(), val));
                        }
                        i += 1;
                    }
                }
                i += 1;
            }
            let cfg = codex_core::config::Config::load_with_cli_overrides(
                kv_overrides,
                codex_core::config::ConfigOverrides::default(),
            )
            .context("load config for --list-models")?;
            if cfg.model_provider_id != "oss" {
                eprintln!("--list-models currently supports --oss (Ollama) provider only.");
                return Ok(());
            }
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .enable_io()
                .build()
                .context("tokio runtime for --list-models")?;
            rt.block_on(async move {
                match codex_ollama::OllamaClient::try_from_oss_provider(&cfg).await {
                    Ok(client) => match client.fetch_models().await {
                        Ok(models) => {
                            for m in models { println!("{}", m); }
                        }
                        Err(e) => eprintln!("error: failed to fetch models: {}", e),
                    },
                    Err(e) => eprintln!("error: {}", e),
                }
            });
            return Ok(());
        }
    }

    // Parse minimal flag: --acp/--ACP. Everything else is forwarded untouched.
    let mut has_acp = false;
    let mut forward: Vec<OsString> = Vec::new();
    // For ACP mode overrides
    let mut overrides: Vec<(String, TomlValue)> = Vec::new();
    let mut list_models = false;
    let mut args = env::args_os().skip(1).peekable();
    while let Some(a) = args.next() {
        if a == "--acp" || a == "--ACP" {
            has_acp = true;
            continue;
        }
        // Parse selected CLI flags to config overrides when in ACP mode.
        if a == "--model" {
            if let Some(v) = args.next() {
                if has_acp {
                    let s = v.to_string_lossy().into_owned();
                    overrides.push(("model".to_string(), TomlValue::String(s)));
                    continue;
                }
                forward.push("--model".into());
                forward.push(v);
                continue;
            }
        }
        if a == "--oss" || a == "--OSS" {
            if has_acp {
                overrides.push(("model_provider".to_string(), TomlValue::String("oss".into())));
                continue;
            }
            forward.push(a);
            continue;
        }
        if a == "--profile" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("profile".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push("--profile".into());
                forward.push(v);
                continue;
            }
        }
        if a == "--cwd" || a == "-C" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("cwd".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--model-provider" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("model_provider".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--model-reasoning-effort" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("model_reasoning_effort".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--model-reasoning-summary" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("model_reasoning_summary".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--model-verbosity" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("model_verbosity".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--hide-agent-reasoning" {
            if has_acp {
                overrides.push(("hide_agent_reasoning".to_string(), TomlValue::Boolean(true)));
                continue;
            }
        }
        if a == "--show-raw-agent-reasoning" {
            if has_acp {
                overrides.push(("show_raw_agent_reasoning".to_string(), TomlValue::Boolean(true)));
                continue;
            }
        }
        if a == "--notify" {
            if let Some(v) = args.next() {
                if has_acp {
                    // Represent as an array with one element: the program. For advanced cases, use -c notify=[...]
                    overrides.push(("notify".to_string(), TomlValue::Array(vec![TomlValue::String(v.to_string_lossy().into_owned())])));
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--reasoning" {
            if let Some(v) = args.next() {
                let vstr = v.to_string_lossy().to_lowercase();
                if has_acp {
                    match vstr.as_str() {
                        "hidden" => {
                            overrides.push(("hide_agent_reasoning".to_string(), TomlValue::Boolean(true)));
                            overrides.push(("show_raw_agent_reasoning".to_string(), TomlValue::Boolean(false)));
                            overrides.push(("model_reasoning_summary".to_string(), TomlValue::String("none".into())));
                        }
                        "summary" => {
                            overrides.push(("hide_agent_reasoning".to_string(), TomlValue::Boolean(false)));
                            overrides.push(("show_raw_agent_reasoning".to_string(), TomlValue::Boolean(false)));
                            overrides.push(("model_reasoning_summary".to_string(), TomlValue::String("concise".into())));
                        }
                        "raw" => {
                            overrides.push(("hide_agent_reasoning".to_string(), TomlValue::Boolean(false)));
                            overrides.push(("show_raw_agent_reasoning".to_string(), TomlValue::Boolean(true)));
                        }
                        _ => {}
                    }
                    continue;
                }
                forward.push(a);
                forward.push(v);
                continue;
            }
        }
        if a == "--sandbox" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("sandbox_mode".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push("--sandbox".into());
                forward.push(v);
                continue;
            }
        }
        if a == "--ask-for-approval" {
            if let Some(v) = args.next() {
                if has_acp {
                    overrides.push(("approval_policy".to_string(), TomlValue::String(v.to_string_lossy().into_owned())));
                    continue;
                }
                forward.push("--ask-for-approval".into());
                forward.push(v);
                continue;
            }
        }
        if a == "--dangerously-bypass-approvals-and-sandbox" {
            if has_acp {
                overrides.push(("approval_policy".to_string(), TomlValue::String("never".into())));
                overrides.push(("sandbox_mode".to_string(), TomlValue::String("danger-full-access".into())));
                continue;
            }
        }
        if a == "--search" || a == "--web-search" {
            if has_acp {
                // tools.web_search = true
                let mut tools: TomlMap<String, TomlValue> = TomlMap::new();
                tools.insert("web_search".to_string(), TomlValue::Boolean(true));
                overrides.push(("tools".to_string(), TomlValue::Table(tools)));
                continue;
            }
        }
        if a == "--list-models" {
            list_models = true;
            continue;
        }
        if a == "-c" || a == "--config" {
            if let Some(kv) = args.next() {
                if has_acp {
                    let s = kv.to_string_lossy();
                    if let Some(eq) = s.find('=') {
                        let (k, vraw) = s.split_at(eq);
                        let vraw = &vraw[1..];
                        let val = toml::from_str::<TomlValue>(vraw).unwrap_or_else(|_| TomlValue::String(vraw.to_string()));
                        overrides.push((k.to_string(), val));
                        continue;
                    }
                }
                forward.push(a);
                forward.push(kv);
                continue;
            }
        }
        // Default: accumulate for upstream
        forward.push(a);
    }

    if has_acp {
        return codex_acp::run_stdio_with_overrides_blocking(overrides);
    }

    // If user asked for help, print upstream help and inject our extra flag.
    let has_help = forward.iter().any(|a| a == "--help" || a == "-h");
    if has_help {
        let output = Command::new("codex")
            .arg("--help")
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .output()
            .context("invoke upstream codex --help")?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        let injection = "      --acp, --ACP  Run ACP stdio server (codex-agentic)\n";
        if let Some(pos) = text.find("Options:") {
            if let Some(nl) = text[pos..].find('\n') {
                let ins = pos + nl + 1;
                text.insert_str(ins, injection);
            } else {
                text.push_str("\nAdditional option (codex-agentic):\n");
                text.push_str(injection);
            }
        } else if let Some(pos) = text.find("Flags:") {
            if let Some(nl) = text[pos..].find('\n') {
                let ins = pos + nl + 1;
                text.insert_str(ins, injection);
            } else {
                text.push_str("\nAdditional option (codex-agentic):\n");
                text.push_str(injection);
            }
        } else {
            text.push_str("\nAdditional option (codex-agentic):\n");
            text.push_str(injection);
        }
        let mut stdout = io::stdout();
        stdout.write_all(text.as_bytes())?;
        stdout.flush()?;
        return Ok(());
    }

    // Handle our optional local helper: --list-models (CLI path).
    if list_models {
        // Load config with collected overrides and query OSS if configured.
        let config = codex_core::config::Config::load_with_cli_overrides(
            overrides.clone(),
            codex_core::config::ConfigOverrides::default(),
        )
        .context("load config for --list-models")?;
        if config.model_provider_id == "oss" {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .enable_io()
                .build()
                .context("tokio runtime for --list-models")?;
            rt.block_on(async move {
                match codex_ollama::OllamaClient::try_from_oss_provider(&config).await {
                    Ok(client) => match client.fetch_models().await {
                        Ok(models) => {
                            for m in models { println!("{}", m); }
                        }
                        Err(e) => eprintln!("error: failed to fetch models: {}", e),
                    },
                    Err(e) => eprintln!("error: {}", e),
                }
            });
            return Ok(());
        } else {
            eprintln!("--list-models currently supports --oss (Ollama) provider only.");
            return Ok(());
        }
    }

    // Default: run our embedded TUI (patched to include OSS models in /model picker)
    let cli = codex_tui::Cli::parse_from(std::iter::once(OsString::from("codex-agentic")).chain(forward.into_iter()));
    let rt = tokio::runtime::Builder::new_multi_thread().enable_io().enable_time().build()?;
    rt.block_on(async move {
        let _ = codex_tui::run_main(cli, None).await;
        Ok(())
    })
}
