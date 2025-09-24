use super::*;
use agent_client_protocol::{AvailableCommand, AvailableCommandInput};
use codex_core::protocol::{AskForApproval, EventMsg, Op, SandboxPolicy, Submission};
use codex_core::protocol_config_types::{ReasoningEffort, ReasoningSummary};
use std::{fs, io};
use tokio::sync::oneshot;

impl CodexAgent {
    pub fn built_in_commands() -> Vec<AvailableCommand> {
        vec![
            AvailableCommand {
                name: "about-codebase".into(),
                description: "Tell me about this codebase (usage: /about-codebase [--refresh|-r])"
                    .into(),
                input: Some(AvailableCommandInput::Unstructured {
                    hint: "[--refresh|-r]".into(),
                }),
                meta: None,
            },
            AvailableCommand {
                name: "init".into(),
                description: "create an AGENTS.md file with instructions for Codex".into(),
                input: None,
                meta: None,
            },
            AvailableCommand {
                name: "thoughts".into(),
                description: "toggle reasoning stream for this session".into(),
                input: Some(AvailableCommandInput::Unstructured {
                    hint: "on|off".into(),
                }),
                meta: None,
            },
            AvailableCommand {
                name: "model".into(),
                description: "choose what model and reasoning effort to use".into(),
                input: Some(AvailableCommandInput::Unstructured {
                    hint: "Model slug, e.g., gpt-codex".into(),
                }),
                meta: None,
            },
            AvailableCommand {
                name: "approvals".into(),
                description: "choose what Codex can do without approval".into(),
                input: Some(AvailableCommandInput::Unstructured {
                    hint: "untrusted|on-request|on-failure|never".into(),
                }),
                meta: None,
            },
            AvailableCommand {
                name: "status".into(),
                description: "show current session configuration and token usage".into(),
                input: None,
                meta: None,
            },
            AvailableCommand {
                name: "reasoning".into(),
                description: "choose how to display reasoning: hidden | summary | raw".into(),
                input: Some(AvailableCommandInput::Unstructured {
                    hint: "hidden|summary|raw".into(),
                }),
                meta: None,
            },
            AvailableCommand {
                name: "index".into(),
                description: "manage local index: /index status | build [--model bge-small|bge-large] [--force] | verify | clean".into(),
                input: Some(AvailableCommandInput::Unstructured { hint: "status|build|verify|clean [args]".into() }),
                meta: None,
            },
            AvailableCommand {
                name: "search".into(),
                description: "semantic search in codebase (local): /search <query> [-k N]".into(),
                input: Some(AvailableCommandInput::Unstructured { hint: "<query> [-k N]".into() }),
                meta: None,
            }

        ]
    }

    pub fn available_commands(&self) -> Vec<AvailableCommand> {
        let mut cmds = Self::built_in_commands();
        cmds.extend(self.extra_available_commands.borrow().iter().cloned());
        cmds
    }

    pub async fn handle_slash_command(
        &self,
        session_id: &SessionId,
        name: &str,
        _rest: &str,
    ) -> Result<bool, Error> {
        let sid_str = session_id.0.to_string();
        let session = match self.sessions.borrow().get(&sid_str) {
            Some(s) => s.clone(),
            None => return Err(Error::invalid_params()),
        };

        // Commands implemented inline (no Codex submission needed)
        match name {
            "about-codebase" => {
                let refresh = {
                    let r = _rest.trim();
                    matches!(r, "--refresh" | "-r" | "refresh")
                        || r.contains(" --refresh")
                        || r.contains(" -r ")
                        || r.ends_with(" --refresh")
                        || r.ends_with(" -r")
                };
                let cwd = self.config.cwd.clone();
                if !refresh {
                    // Quick view: render saved report if available; do NOT route via LLM.
                    if let Ok(rep) = crate::review_persist::load_previous_report_sync(&cwd) {
                        if rep.report.markdown.trim().is_empty() {
                            let msg = "No saved report content yet — run /about-codebase --refresh to generate one.";
                            let (tx, rx) = oneshot::channel();
                            self.send_message_chunk(session_id, msg.into(), tx)?;
                            let _ = rx.await;
                            return Ok(true);
                        } else {
                            let sanitized = crate::review_persist::sanitize_markdown_for_display(
                                &rep.report.markdown,
                            );
                            // Display the saved report to the client
                            let (tx, rx) = oneshot::channel();
                            self.send_message_chunk(session_id, sanitized.clone().into(), tx)?;
                            let _ = rx.await;
                            return Ok(true);
                        }
                    } else {
                        // First run: inform and fall through to refresh behavior
                        let (tx, rx) = oneshot::channel();
                        self.send_message_chunk(
                            session_id,
                            "First time running code check — generating the report…".into(),
                            tx,
                        )?;
                        let _ = rx.await;
                        let (tx, rx) = oneshot::channel();
                        self.send_message_chunk(
                            session_id,
                            "Please wait, this may take some time :)".into(),
                            tx,
                        )?;
                        let _ = rx.await;
                        // Treat as refresh
                    }
                }

                // Refresh: require a Codex conversation
                let sid_str = session_id.0.to_string();
                let session = self
                    .sessions
                    .borrow()
                    .get(&sid_str)
                    .cloned()
                    .ok_or_else(Error::invalid_params)?;
                let Some(conv) = session.conversation.as_ref() else {
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(
                        session_id,
                        "Dev mock mode: refresh requires Codex backend".into(),
                        tx,
                    )?;
                    let _ = rx.await;
                    return Ok(true);
                };

                // Assemble a compact prompt (ACP minimal version)
                let prompt = {
                    let mut p = String::new();
                    use std::fmt::Write as _;
                    let _ = writeln!(p, "# /about-codebase");
                    let _ = writeln!(p, "Please produce a concise, high-signal codebase review.");
                    let _ = writeln!(
                        p,
                        "Focus on: Architecture, Important Flows, CI/Release, Config & Env, Design Choices, Risks."
                    );
                    let _ = writeln!(p);
                    let _ = writeln!(p, "Workspace: {}", cwd.display());
                    let _ = writeln!(p);
                    let _ = writeln!(p, "Return Markdown with those section headers.");
                    p
                };

                let submit_id = format!("s{}-{}", sid_str, self.next_submit_seq.get());
                self.next_submit_seq.set(self.next_submit_seq.get() + 1);
                conv.submit_with_id(Submission {
                    id: submit_id.clone(),
                    op: Op::UserInput {
                        items: vec![InputItem::Text { text: prompt }],
                    },
                })
                .await
                .map_err(Error::into_internal_error)?;

                let mut acc = String::new();
                loop {
                    let event = conv
                        .next_event()
                        .await
                        .map_err(Error::into_internal_error)?;
                    if event.id != submit_id {
                        continue;
                    }
                    match event.msg {
                        EventMsg::AgentMessageDelta(delta) => {
                            let mut chunk = delta.delta;
                            if crate::review_persist::needs_newline_before_heading(&acc, &chunk) {
                                chunk = format!("\n\n{}", chunk);
                            }
                            acc.push_str(&chunk);
                            let (tx, rx) = oneshot::channel();
                            self.send_message_chunk(session_id, chunk.into(), tx)?;
                            let _ = rx.await;
                        }
                        EventMsg::AgentMessage(_) => {}
                        EventMsg::TaskComplete(_) | EventMsg::ShutdownComplete => {
                            // Persist the final markdown
                            let model = session.current_model.clone();
                            if !acc.trim().is_empty() {
                                if let Err(e) = crate::review_persist::update_report_markdown_sync(
                                    &cwd,
                                    &acc,
                                    Some(model),
                                ) {
                                    let (tx, rx) = oneshot::channel();
                                    self.send_message_chunk(
                                        session_id,
                                        format!("Failed to save report: {e}").into(),
                                        tx,
                                    )?;
                                    let _ = rx.await;
                                } else {
                                    let (tx, rx) = oneshot::channel();
                                    self.send_message_chunk(session_id, "Saved updated codebase report to .codex/review-codebase.json".into(), tx)?;
                                    let _ = rx.await;
                                }
                            }
                            break;
                        }
                        EventMsg::Error(err) => {
                            let (tx, rx) = oneshot::channel();
                            self.send_message_chunk(session_id, err.message.into(), tx)?;
                            let _ = rx.await;
                            break;
                        }
                        _ => {}
                    }
                }
                return Ok(true);
            }
            "init" => {
                // Create AGENTS.md in the current workspace if it doesn't already exist.
                let rest = _rest.trim();
                let force = matches!(rest, "--force" | "-f" | "force");

                let cwd = self.config.cwd.clone();
                // If any AGENTS* file already exists and not forcing, bail out.
                let existing = self.find_agents_files();
                if !existing.is_empty() && !force {
                    let msg = format!(
                        "AGENTS file already exists: {}\nUse /init --force to overwrite.",
                        existing.join(", ")
                    );

                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                    return Ok(true);
                }

                let target = cwd.join("AGENTS.md");
                let template = r#"# AGENTS.md

This file gives Codex instructions for working in this repository. Place project-specific tips here so the agent acts consistently with your workflows.

Scope
- The scope of this file is the entire repository (from this folder down).
- Add more AGENTS.md files in subdirectories for overrides; deeper files take precedence.

Coding Conventions
- Keep changes minimal and focused on the task.
- Match the existing code style and structure; avoid wholesale refactors.
- Don't add licenses or headers unless requested.

Formatting (Markdown)
- Always respond in valid Markdown.
- Use headings (## ...), short bullet lists, and fenced code blocks with language tags (```bash, ```json, ```rust, etc.).
- Do not mix prose and code on the same line; put code in fenced blocks only.
- Keep answers concise and actionable; avoid preambles and meta commentary.
- For long answers, include a short Summary section at the end.

Reasoning / "Thinking"
- Do not output chain-of-thought by default. Provide final answers and brief bullet rationales only.
- Prefer succinct explanations; avoid step-by-step internal reasoning unless explicitly requested.

Workflow
- How to run and test: describe commands (e.g., `cargo test`, `npm test`).
- Any environment variables or secrets required for local runs.
- Where to place new modules, configs, or scripts.

Reviews and Safety
- Point out risky or destructive actions before performing them.
- Prefer root-cause fixes over band-aids.
- When in doubt, ask for confirmation.

Notes for Agents
- Follow instructions in this file for all edits within its scope.
- Files in deeper directories with their own AGENTS.md override these rules.
"#;

                // Try to write the file; on errors, surface a message.
                let result = (|| -> io::Result<()> {
                    // Ensure parent exists (workspace root should exist already).
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&target, template)
                })();

                let msg = match result {
                    Ok(()) => format!(
                        "Initialized AGENTS.md at {}\nEdit it to customize agent behavior.",
                        self.shorten_home(&target)
                    ),
                    Err(e) => format!(
                        "Failed to create AGENTS.md: {}\nPath: {}",
                        e,
                        self.shorten_home(&target)
                    ),
                };

                let (tx, rx) = oneshot::channel();
                self.send_message_chunk(session_id, msg.into(), tx)?;
                let _ = rx.await;
                return Ok(true);
            }
            "thoughts" => {
                let arg = _rest.trim().to_lowercase();
                let desired = match arg.as_str() {
                    "on" => Some(true),
                    "off" => Some(false),
                    _ => None,
                };

                if let Some(v) = desired {
                    if let Ok(mut map) = self.sessions.try_borrow_mut()
                        && let Some(state) = map.get_mut(&sid_str)
                    {
                        state.show_reasoning = v;
                    }
                    let msg = if v {
                        "Reasoning stream: `on`"
                    } else {
                        "Reasoning stream: `off` (thinking minimized)"
                    };
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                    return Ok(true);
                } else {
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, "Usage: //thoughts on|off".into(), tx)?;
                    let _ = rx.await;
                    return Ok(true);
                }
            }
            "index" => {
                let args = _rest.trim();
                let mut cli: Vec<String> = vec!["index".into()];
                if args.is_empty() {
                    cli.push("status".into());
                } else {
                    cli.extend(args.split_whitespace().map(|s| s.to_string()));
                }
                let out = run_codex_agentic(cli).await;
                let (tx, rx) = oneshot::channel();
                self.send_message_chunk(
                    session_id,
                    format!(
                        "```text
{}
```",
                        out
                    )
                    .into(),
                    tx,
                )?;
                let _ = rx.await;
                return Ok(true);
            }
            "search" => {
                let rest = _rest.trim();
                if rest.is_empty() {
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(
                        session_id,
                        "Usage: /search <query> [-k N]".into(),
                        tx,
                    )?;
                    let _ = rx.await;
                    return Ok(true);
                }
                let mut cli: Vec<String> = vec!["index".into(), "query".into(), rest.into()];
                if !rest.contains("-k ") {
                    cli.push("-k".into());
                    cli.push("8".into());
                }
                cli.push("--show-snippets".into());
                let out = run_codex_agentic(cli).await;
                let (tx, rx) = oneshot::channel();
                self.send_message_chunk(
                    session_id,
                    format!(
                        "```text
{}
```",
                        out
                    )
                    .into(),
                    tx,
                )?;
                let _ = rx.await;
                return Ok(true);
            }
            "status" => {
                let status_text = self.render_status(&sid_str).await;
                let (tx, rx) = oneshot::channel();
                self.send_message_chunk(session_id, status_text.into(), tx)?;
                let _ = rx.await;
                return Ok(true);
            }
            "reasoning" => {
                let arg = _rest.trim().to_lowercase();
                let sid_str = session_id.0.to_string();
                let summary = match arg.as_str() {
                    "hidden" => Some(ReasoningSummary::None),
                    "summary" => Some(ReasoningSummary::Concise),
                    "raw" => Some(ReasoningSummary::Auto),
                    _ => None,
                };
                if let Some(summary) = summary {
                    let submit_id = format!("s{}-{}", sid_str, self.next_submit_seq.get());
                    self.next_submit_seq.set(self.next_submit_seq.get() + 1);
                    let op = Op::OverrideTurnContext {
                        cwd: None,
                        approval_policy: None,
                        sandbox_policy: None,
                        model: None,
                        effort: None,
                        summary: Some(summary),
                    };
                    if let Some(conv) = session.conversation.as_ref() {
                        conv.submit_with_id(Submission { id: submit_id, op })
                            .await
                            .map_err(Error::into_internal_error)?;
                    }
                    let msg = format!("Reasoning set to: {}", arg);
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                } else {
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(
                        session_id,
                        "Usage: /reasoning hidden|summary|raw".into(),
                        tx,
                    )?;
                    let _ = rx.await;
                }
                return Ok(true);
            }
            "model" => {
                let rest = _rest.trim();
                if rest.is_empty() {
                    let msg = format!(
                        "Current model: {}\nUsage: /model <model-slug> [low|medium|high]",
                        self.config.model,
                    );
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                    return Ok(true);
                }

                // Parse model and optional reasoning effort
                let parts: Vec<&str> = rest.split_whitespace().collect();
                let model_name = parts[0].to_string();
                let effort = if parts.len() > 1 {
                    match parts[1].to_lowercase().as_str() {
                        "low" => Some(ReasoningEffort::Low),
                        "medium" => Some(ReasoningEffort::Medium),
                        "high" => Some(ReasoningEffort::High),
                        _ => None,
                    }
                } else {
                    None
                };

                // Update session state
                {
                    let mut sessions = self.sessions.borrow_mut();
                    if let Some(state) = sessions.get_mut(&sid_str) {
                        state.current_model = model_name.clone();
                        if let Some(e) = effort {
                            state.current_effort = Some(e);
                        }
                    }
                }

                // Request Codex to change the model for subsequent turns.
                let submit_id = format!("s{}-{}", sid_str, self.next_submit_seq.get());
                self.next_submit_seq.set(self.next_submit_seq.get() + 1);
                let op = Op::OverrideTurnContext {
                    cwd: None,
                    approval_policy: None,
                    sandbox_policy: None,
                    model: Some(model_name.clone()),
                    effort: effort.map(Some),
                    summary: None,
                };
                if let Some(conv) = session.conversation.as_ref() {
                    conv.submit_with_id(Submission { id: submit_id, op })
                        .await
                        .map_err(Error::into_internal_error)?;
                } else {
                    let msg = "Dev mock mode: /model not available without Codex backend";
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                    return Ok(true);
                }

                // Show updated status after model change
                let status_text = self.render_status(&sid_str).await;
                let (tx, rx) = oneshot::channel();
                self.send_message_chunk(session_id, status_text.into(), tx)?;
                let _ = rx.await;
                return Ok(true);
            }
            "approvals" => {
                let value = _rest.trim().to_lowercase();
                let parsed = match value.as_str() {
                    "" | "show" => None,
                    "on-request" => Some(AskForApproval::OnRequest),
                    "on-failure" => Some(AskForApproval::OnFailure),
                    "never" => Some(AskForApproval::Never),
                    "untrusted" | "unless-trusted" => Some(AskForApproval::UnlessTrusted),
                    _ => {
                        let msg = "Usage: /approvals untrusted|on-request|on-failure|never";
                        let (tx, rx) = oneshot::channel();
                        self.send_message_chunk(session_id, msg.into(), tx)?;
                        let _ = rx.await;
                        return Ok(true);
                    }
                };

                if let Some(policy) = parsed {
                    let submit_id = format!("s{}-{}", sid_str, self.next_submit_seq.get());
                    self.next_submit_seq.set(self.next_submit_seq.get() + 1);
                    let op = Op::OverrideTurnContext {
                        cwd: None,
                        approval_policy: Some(policy),
                        sandbox_policy: None,
                        model: None,
                        effort: None,
                        summary: None,
                    };
                    if let Some(conv) = session.conversation.as_ref() {
                        conv.submit_with_id(Submission { id: submit_id, op })
                            .await
                            .map_err(Error::into_internal_error)?;
                    } else {
                        let msg = "Dev mock mode: /approvals requires Codex backend";
                        let (tx, rx) = oneshot::channel();
                        self.send_message_chunk(session_id, msg.into(), tx)?;
                        let _ = rx.await;
                        return Ok(true);
                    }
                    // Persist our local view of the policy for /status
                    if let Ok(mut map) = self.sessions.try_borrow_mut()
                        && let Some(state) = map.get_mut(&sid_str)
                    {
                        state.current_approval = policy;
                    }
                    let msg = format!("Approval policy set to: {}", value);
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                } else {
                    // show current (best-effort from config)
                    let msg = "Current approval policy: configured per session. Use /approvals <policy> to set.";
                    let (tx, rx) = oneshot::channel();
                    self.send_message_chunk(session_id, msg.into(), tx)?;
                    let _ = rx.await;
                }
                return Ok(true);
            }
            _ => {}
        }

        // Commands forwarded to Codex as protocol Ops
        let op = match name {
            "compact" => Some(Op::Compact),
            "list-tools" | "tools" => Some(Op::ListMcpTools),
            "list-custom-prompts" | "prompts" => Some(Op::ListCustomPrompts),
            "history" => Some(Op::GetPath),
            "shutdown" => Some(Op::Shutdown),
            _ => None,
        };

        if let Some(op) = op {
            let submit_id = format!("s{}-{}", sid_str, self.next_submit_seq.get());
            self.next_submit_seq.set(self.next_submit_seq.get() + 1);
            if let Some(conv) = session.conversation.as_ref() {
                conv.submit_with_id(Submission {
                    id: submit_id.clone(),
                    op,
                })
                .await
                .map_err(Error::into_internal_error)?;
            } else {
                let msg = "Dev mock mode: command requires Codex backend";
                let (tx, rx) = oneshot::channel();
                self.send_message_chunk(session_id, msg.into(), tx)?;
                let _ = rx.await;
                return Ok(true);
            }

            // Stream events for this submission using the same loop as in prompt
            loop {
                let event = session
                    .conversation
                    .as_ref()
                    .unwrap()
                    .next_event()
                    .await
                    .map_err(Error::into_internal_error)?;
                if event.id != submit_id {
                    continue;
                }
                match event.msg {
                    EventMsg::AgentMessageDelta(delta) => {
                        let (tx, rx) = oneshot::channel();
                        self.session_update_tx
                            .send((
                                SessionNotification {
                                    session_id: session_id.clone(),
                                    update: SessionUpdate::AgentMessageChunk {
                                        content: delta.delta.into(),
                                    },
                                    meta: None,
                                },
                                tx,
                            ))
                            .map_err(Error::into_internal_error)?;
                        let _ = rx.await;
                    }
                    EventMsg::AgentMessage(_msg) => {
                        // Skip complete message since we're already sending deltas
                        // This prevents duplicate text in the chat interface
                    }
                    EventMsg::TaskComplete(_) | EventMsg::ShutdownComplete => {
                        break;
                    }
                    EventMsg::Error(err) => {
                        let (tx, rx) = oneshot::channel();
                        self.send_message_chunk(session_id, err.message.into(), tx)?;
                        let _ = rx.await;
                        break;
                    }
                    _ => {}
                }
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) async fn render_status(&self, sid_str: &str) -> String {
        // Session snapshot
        let (approval_mode, sandbox_mode, token_usage, session_uuid, current_model, current_effort) = {
            let map = self.sessions.borrow();
            if let Some(state) = map.get(sid_str) {
                (
                    state.current_approval,
                    state.current_sandbox.clone(),
                    state.token_usage.clone(),
                    state.conversation_id.clone(),
                    state.current_model.clone(),
                    state.current_effort,
                )
            } else {
                (
                    AskForApproval::OnRequest,
                    SandboxPolicy::new_workspace_write_policy(),
                    None,
                    String::new(),
                    self.config.model.clone(),
                    self.config.model_reasoning_effort,
                )
            }
        };

        // Workspace
        let cwd = self.shorten_home(&self.config.cwd);
        let agents_files = self.find_agents_files();
        let agents_line = if agents_files.is_empty() {
            "(none)".to_string()
        } else {
            agents_files.join(", ")
        };

        // Account
        let (auth_mode, email, plan): (String, String, String) =
            match self.auth_manager.read().ok().and_then(|am| am.auth()) {
                Some(auth) => match auth.get_token_data().await {
                    Ok(td) => {
                        let email = td
                            .id_token
                            .email
                            .clone()
                            .unwrap_or_else(|| "(none)".to_string());
                        let plan = td
                            .id_token
                            .get_chatgpt_plan_type()
                            .unwrap_or_else(|| "(unknown)".to_string());
                        ("ChatGPT".to_string(), email, plan)
                    }
                    Err(_) => (
                        "API key".to_string(),
                        "(none)".to_string(),
                        "(unknown)".to_string(),
                    ),
                },
                None => (
                    "Not signed in".to_string(),
                    "(none)".to_string(),
                    "(unknown)".to_string(),
                ),
            };

        // Model
        let model = &current_model;
        let provider = self.title_case(&self.config.model_provider_id);
        let effort = format!("{:?}", current_effort);
        let summary = format!("{:?}", self.config.model_reasoning_summary);

        let reasoning_on = {
            let map = self.sessions.borrow();
            map.get(sid_str).map(|s| s.show_reasoning).unwrap_or(false)
        };

        // Tokens
        let (input, output, total) = match token_usage {
            Some(u) => (u.input_tokens, u.output_tokens, u.total_tokens),
            None => (0, 0, 0),
        };

        // Compute YOLO indicator
        let yolo = matches!(approval_mode, AskForApproval::Never)
            && matches!(sandbox_mode, SandboxPolicy::DangerFullAccess);
        let web_search = self.config.tools_web_search_request;

        // Index status (best-effort)
        let index_status = run_codex_agentic(vec!["index".into(), "status".into()]).await;

        // Markdown output with headings and lists
        format!(
            concat!(
                "## Workspace\n",
                "- Path: `{cwd}`\n",
                "- Approval Mode: `{approval}`\n",
                "- Sandbox: `{sandbox}`\n",
                "- YOLO with search: `{yolo_with_search}`\n",
                "- AGENTS files: {agents}\n\n",
                "## Account\n",
                "- Signed in with: `{auth_mode}`\n",
                "- Login: `{email}`\n",
                "- Plan: `{plan}`\n\n",
                "## Model\n",
                "- Name: `{model}`\n",
                "- Provider: `{provider}`\n",
                "- Reasoning Effort: `{effort}`\n",
                "- Reasoning Summaries: `{summary}`\n",
                "- Reasoning Stream: `{reasoning}`\n\n",
                "## Token Usage\n",
                "- Session ID: `{sid}`\n",
                "- Input: `{input}`\n",
                "- Output: `{output}`\n",
                "- Total: `{total}`\n\n",
                "## Index\n",
                "```text\n{index_status}\n```\n"
            ),
            cwd = cwd,
            approval = approval_mode,
            sandbox = sandbox_mode,
            agents = agents_line,
            yolo_with_search = if yolo && web_search { "on" } else { "off" },
            auth_mode = auth_mode,
            email = email,
            plan = plan,
            model = model,
            provider = provider,
            effort = self.title_case(&effort),
            summary = self.title_case(&summary),
            sid = session_uuid,
            reasoning = if reasoning_on { "on" } else { "off" },
            input = input,
            output = output,
            total = total,
            index_status = index_status.trim(),
        )
    }

    fn shorten_home(&self, p: &std::path::Path) -> String {
        let s = p.display().to_string();
        if let Ok(home) = std::env::var("HOME")
            && s.starts_with(&home)
        {
            return s.replacen(&home, "~", 1);
        }
        s
    }

    fn find_agents_files(&self) -> Vec<String> {
        let mut names = Vec::new();
        let candidates = ["AGENTS.md", "Agents.md", "agents.md"];
        for c in candidates.iter() {
            let path = self.config.cwd.join(c);
            if path.exists() {
                names.push(c.to_string());
            }
        }
        names
    }

    fn title_case(&self, s: &str) -> String {
        if s.is_empty() {
            return s.to_string();
        }
        let mut chars = s.chars();
        let first = chars.next().unwrap().to_uppercase().to_string();
        let rest = chars.as_str();
        format!("{}{}", first, rest)
    }
}

async fn run_codex_agentic(args: Vec<String>) -> String {
    use tokio::process::Command;
    let mut cmd = Command::new("codex-agentic");
    for a in args {
        cmd.arg(a);
    }
    match cmd.output().await {
        Ok(o) => {
            let mut s = String::new();
            if !o.stdout.is_empty() {
                s.push_str(&String::from_utf8_lossy(&o.stdout));
            }
            if !o.stderr.is_empty() {
                if !s.is_empty() {
                    s.push_str(
                        "
",
                    );
                }
                s.push_str(&String::from_utf8_lossy(&o.stderr));
            }
            if s.trim().is_empty() {
                "(no output)".into()
            } else {
                s
            }
        }
        Err(e) => format!("Failed to run codex-agentic: {}", e),
    }
}
