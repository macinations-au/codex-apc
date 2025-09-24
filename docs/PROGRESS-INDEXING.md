# Progress — Codebase Indexing (Local‑Only)

Version: 0.39.0‑apc.8

What’s landed
- CLI indexer with incremental git‑delta rebuild (flat vectors + meta, atomic swap)
- Background maintenance (5‑min timer)
- ACP: in‑process retrieval (FastEmbed OnceLock + mmap + rayon)
- ACP: always show references summary before LLM; inject context only when top score ≥ threshold (default 95%)
- TUI: `/status` shows Index section; retrieval still shell‑out (port pending)

Where to continue
- Implement HNSW on index build; persist graph; prefer HNSW on CLI query
- Port ACP in‑process path to TUI (and references summary)
- Replace refresh timer with post‑turn trigger; cap work/backoff
- Token‑aware context budget; JSON `index query` output; second grammar (TS/Python)

Paths of interest
- codex-agentic/src/indexing/mod.rs (build/query/status/verify)
- codex-acp/src/agent.rs (fetch_retrieval_context + injection call site)
- codex-tui/src/chatwidget.rs (submit_user_message)

Env flags
- CODEX_INDEXING=0 — disable auto build/refresh
- CODEX_INDEX_RETRIEVAL=0 — disable retrieval injection
- CODEX_INDEX_RETRIEVAL_THRESHOLD=0.95 — adjust confidence
