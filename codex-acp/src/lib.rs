use anyhow::Result;
use tokio::{io, sync::mpsc, task};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

use agent_client_protocol::{AgentSideConnection, Client};

mod agent;
pub use crate::agent::CodexAgent;

use codex_core::config::{Config, ConfigOverrides};

/// Run the ACP agent over stdio using the provided `Config`.
///
/// This is the primary entry point for embedding into other binaries (e.g., codex-cli).
/// It performs no global tracing initialization — callers control logging.
pub async fn run_stdio_with_config(config: Config) -> Result<()> {
    let outgoing = io::stdout().compat_write();
    let incoming = io::stdin().compat();

    let local_set = task::LocalSet::new();
    local_set
        .run_until(async move {
            let (tx, mut rx) = mpsc::unbounded_channel();
            let (client_tx, mut client_rx) = mpsc::unbounded_channel();

            let agent = CodexAgent::with_config(tx, client_tx.clone(), config);
            let (conn, handle_io) = AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                task::spawn_local(fut);
            });

            // Bridge internal channels to ACP connection
            task::spawn_local(async move {
                loop {
                    tokio::select! {
                        msg = rx.recv() => {
                            match msg {
                                Some((session_notification, tx)) => {
                                    let result = conn.session_notification(session_notification).await;
                                    if result.is_err() { break; }
                                    let _ = tx.send(());
                                }
                                None => break,
                            }
                        }
                        op = client_rx.recv() => {
                            match op {
                                Some(agent::ClientOp::RequestPermission(req, tx)) => {
                                    let res = conn.request_permission(req).await;
                                    let _ = tx.send(res);
                                }
                                None => break,
                            }
                        }
                    }
                }
            });

            handle_io.await
        })
        .await
}

/// Run the ACP agent over stdio with CLI-style key/value overrides applied.
pub async fn run_stdio_with_overrides(
    overrides: Vec<(String, toml::Value)>,
    cfg_over: ConfigOverrides,
) -> Result<()> {
    let config = Config::load_with_cli_overrides(overrides, cfg_over)?;
    run_stdio_with_config(config).await
}

/// Blocking helper: apply overrides and run stdio mode.
pub fn run_stdio_with_overrides_blocking(overrides: Vec<(String, toml::Value)>) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()?;
    rt.block_on(async {
        run_stdio_with_overrides(overrides, ConfigOverrides::default()).await
    })
}

/// Load configuration using Codex defaults and optional CLI-style overrides, then
/// run the ACP agent over stdio.
/// Convenience blocking helper that sets up a single-threaded runtime and runs stdio mode.
/// Suitable for embedding into non-async entrypoints.
pub fn run_stdio_blocking() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .enable_io()
        .build()?;
    rt.block_on(async {
        let config = Config::load_with_cli_overrides(vec![], ConfigOverrides::default())?;
        run_stdio_with_config(config).await
    })
}
