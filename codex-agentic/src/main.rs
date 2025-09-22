use anyhow::{Context, Result};
use clap::Parser as _;
use codex_core::config::ConfigOverrides as CoreConfigOverrides;
use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use toml::Value as TomlValue;
use toml::map::Map as TomlMap;

fn main() -> Result<()> {
    // Early fast-path: support `--list-models` in CLI mode before any passthrough.
    {
        let args_utf: Vec<String> = env::args().skip(1).collect();
        if args_utf.iter().any(|a| a == "--list-models") {
            // Build minimal overrides: honor --oss/--OSS and -c/--config pairs
            let mut kv_overrides: Vec<(String, TomlValue)> = Vec::new();
            if args_utf.iter().any(|a| a == "--oss" || a == "--OSS") {
                kv_overrides.push((
                    "model_provider".to_string(),
                    TomlValue::String("oss".into()),
                ));
            }
            let mut i = 0;
            while i < args_utf.len() {
                let a = &args_utf[i];
                if (a == "-c" || a == "--config") && i + 1 < args_utf.len() {
                    let s = &args_utf[i + 1];
                    if let Some(eq) = s.find('=') {
                        let (k, vraw) = s.split_at(eq);
                        let vraw = &vraw[1..];
                        let val = toml::from_str::<TomlValue>(vraw)
                            .unwrap_or_else(|_| TomlValue::String(vraw.to_string()));
                        kv_overrides.push((k.to_string(), val));
                    }
                    i += 1;
                }
                i += 1;
            }
            // Treat provider selection as session-only: prefer typed overrides
            // instead of writing a key/value that might be persisted by
            // downstream helpers. Only set provider via typed override here.
            let mut typed_over = CoreConfigOverrides::default();
            if kv_overrides.iter().any(|(k, _)| k == "model_provider") {
                // Remove any accidental KV provider override
                let kv_overrides: Vec<(String, TomlValue)> = kv_overrides
                    .into_iter()
                    .filter(|(k, _)| k != "model_provider")
                    .collect();
                typed_over.model_provider =
                    Some(codex_core::BUILT_IN_OSS_MODEL_PROVIDER_ID.to_string());
                let cfg =
                    codex_core::config::Config::load_with_cli_overrides(kv_overrides, typed_over)
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
                                for m in models {
                                    println!("{}", m);
                                }
                            }
                            Err(e) => eprintln!("error: failed to fetch models: {}", e),
                        },
                        Err(e) => eprintln!("error: {}", e),
                    }
                });
            } else {
                let cfg =
                    codex_core::config::Config::load_with_cli_overrides(kv_overrides, typed_over)
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
                                for m in models {
                                    println!("{}", m);
                                }
                            }
                            Err(e) => eprintln!("error: failed to fetch models: {}", e),
                        },
                        Err(e) => eprintln!("error: {}", e),
                    }
                });
            }
            return Ok(());
        }
    }

    // Parse minimal flag: --acp/--ACP. Everything else is forwarded untouched.
    let mut has_acp = false;
    let mut forward: Vec<OsString> = Vec::new();
    // For ACP mode overrides
    let mut overrides: Vec<(String, TomlValue)> = Vec::new();
    // Typed overrides for ACP (session-only to avoid persisting provider)
    let mut typed_overrides: CoreConfigOverrides = CoreConfigOverrides::default();
    let mut list_models = false;
    let mut args = env::args_os().skip(1).peekable();
    while let Some(a) = args.next() {
        if a == "--acp" || a == "--ACP" {
            has_acp = true;
            continue;
        }
        // Parse selected CLI flags to config overrides when in ACP mode.
        if a == "--model"
            && let Some(v) = args.next()
        {
            if has_acp {
                let s = v.to_string_lossy().into_owned();
                overrides.push(("model".to_string(), TomlValue::String(s)));
                continue;
            }
            forward.push("--model".into());
            forward.push(v);
            continue;
        }
        if a == "--oss" || a == "--OSS" {
            if has_acp {
                // Use typed override so provider stays session-only; avoid KV that
                // could be interpreted as persistent.
                typed_overrides.model_provider =
                    Some(codex_core::BUILT_IN_OSS_MODEL_PROVIDER_ID.to_string());
                continue;
            }
            forward.push(a);
            continue;
        }
        if a == "--profile"
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "profile".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push("--profile".into());
            forward.push(v);
            continue;
        }
        if (a == "--cwd" || a == "-C")
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "cwd".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--model-provider"
            && let Some(v) = args.next()
        {
            if has_acp {
                // Respect explicit model-provider flag via KV override.
                // (We only special-case --oss for session-only behavior.)
                overrides.push((
                    "model_provider".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--model-reasoning-effort"
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "model_reasoning_effort".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--model-reasoning-summary"
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "model_reasoning_summary".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--model-verbosity"
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "model_verbosity".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--hide-agent-reasoning" && has_acp {
            overrides.push(("hide_agent_reasoning".to_string(), TomlValue::Boolean(true)));
            continue;
        }
        if a == "--show-raw-agent-reasoning" && has_acp {
            overrides.push((
                "show_raw_agent_reasoning".to_string(),
                TomlValue::Boolean(true),
            ));
            continue;
        }
        if a == "--notify"
            && let Some(v) = args.next()
        {
            if has_acp {
                // Represent as an array with one element: the program. For advanced cases, use -c notify=[...]
                overrides.push((
                    "notify".to_string(),
                    TomlValue::Array(vec![TomlValue::String(v.to_string_lossy().into_owned())]),
                ));
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--reasoning"
            && let Some(v) = args.next()
        {
            let vstr = v.to_string_lossy().to_lowercase();
            if has_acp {
                match vstr.as_str() {
                    "hidden" => {
                        overrides
                            .push(("hide_agent_reasoning".to_string(), TomlValue::Boolean(true)));
                        overrides.push((
                            "show_raw_agent_reasoning".to_string(),
                            TomlValue::Boolean(false),
                        ));
                        overrides.push((
                            "model_reasoning_summary".to_string(),
                            TomlValue::String("none".into()),
                        ));
                    }
                    "summary" => {
                        overrides.push((
                            "hide_agent_reasoning".to_string(),
                            TomlValue::Boolean(false),
                        ));
                        overrides.push((
                            "show_raw_agent_reasoning".to_string(),
                            TomlValue::Boolean(false),
                        ));
                        overrides.push((
                            "model_reasoning_summary".to_string(),
                            TomlValue::String("concise".into()),
                        ));
                    }
                    "raw" => {
                        overrides.push((
                            "hide_agent_reasoning".to_string(),
                            TomlValue::Boolean(false),
                        ));
                        overrides.push((
                            "show_raw_agent_reasoning".to_string(),
                            TomlValue::Boolean(true),
                        ));
                    }
                    _ => {}
                }
                continue;
            }
            forward.push(a);
            forward.push(v);
            continue;
        }
        if a == "--sandbox"
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "sandbox_mode".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push("--sandbox".into());
            forward.push(v);
            continue;
        }
        if a == "--ask-for-approval"
            && let Some(v) = args.next()
        {
            if has_acp {
                overrides.push((
                    "approval_policy".to_string(),
                    TomlValue::String(v.to_string_lossy().into_owned()),
                ));
                continue;
            }
            forward.push("--ask-for-approval".into());
            forward.push(v);
            continue;
        }
        if a == "--dangerously-bypass-approvals-and-sandbox" && has_acp {
            overrides.push((
                "approval_policy".to_string(),
                TomlValue::String("never".into()),
            ));
            overrides.push((
                "sandbox_mode".to_string(),
                TomlValue::String("danger-full-access".into()),
            ));
            continue;
        }
        if (a == "--search" || a == "--web-search") && has_acp {
            // tools.web_search = true
            let mut tools: TomlMap<String, TomlValue> = TomlMap::new();
            tools.insert("web_search".to_string(), TomlValue::Boolean(true));
            overrides.push(("tools".to_string(), TomlValue::Table(tools)));
            continue;
        }
        if a == "--list-models" {
            list_models = true;
            continue;
        }
        if (a == "-c" || a == "--config")
            && let Some(kv) = args.next()
        {
            if has_acp {
                let s = kv.to_string_lossy();
                if let Some(eq) = s.find('=') {
                    let (k, vraw) = s.split_at(eq);
                    let vraw = &vraw[1..];
                    let val = toml::from_str::<TomlValue>(vraw)
                        .unwrap_or_else(|_| TomlValue::String(vraw.to_string()));
                    overrides.push((k.to_string(), val));
                    continue;
                }
            }
            forward.push(a);
            forward.push(kv);
            continue;
        }
        // Default: accumulate for upstream
        forward.push(a);
    }

    if has_acp {
        // Run ACP with both KV and typed overrides (session-only provider).
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .enable_io()
            .build()
            .context("tokio runtime for ACP stdio")?;
        return rt.block_on(async move {
            codex_acp::run_stdio_with_overrides(overrides, typed_overrides).await
        });
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
                            for m in models {
                                println!("{}", m);
                            }
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
    let cli =
        codex_tui::Cli::parse_from(std::iter::once(OsString::from("codex-agentic")).chain(forward));
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_io()
        .enable_time()
        .build()?;
    rt.block_on(async move {
        let _ = codex_tui::run_main(cli, None).await;
        Ok(())
    })
}
