## Codex Agentic — Codebase Indexing Technical Specification (Local‑Only)

Version: 0.1 (2025-09-24)
Owners: codex-agentic maintainers
Status: Ready for implementation

---

## 1) Scope & Goals
- Fully local semantic indexing for codebases to improve retrieval for chat.
- Default ON on first run (opt‑out available); non-blocking if failures occur.
- Tree-sitter–based chunking by default (Rust grammar in MVP), with robust fallbacks.
- Fast, durable on-disk ANN index + metadata; verification and atomic analytics.

Non‑Goals (MVP)
- External providers, vector DB services, file watchers, multi-repo federated search.

---

## 2) Architecture Overview
- Library (internal module for MVP; may split into a crate later): `codex_index`.
- Components:
  - Scanner: walks repo respecting .gitignore and deny-lists.
  - BinaryGuard: magic-bytes + UTF‑8 probe to skip binaries.
  - Chunker: `tree-sitter` (Rust) → blank-line → line-window.
  - Embedder: `fastembed` CPU ONNX models (default bge-small 384‑D; optional bge-large 1024‑D).
  - IndexStore: HNSW ANN with cosine/IP, persisted to disk.
  - MetaStore: JSONL per chunk (path, lines, lang, sha256, preview).
  - Manifest: JSON describing engine/model/dim/config/counts/checksums.
  - Verifier: checks manifest compatibility + file checksums; `index verify`.
  - Analytics: atomic counters in `analytics.json` (queries/hits/misses/last_query_ts).
  - Retriever: embed(query) → top‑K → snippets.

Data Flow
1) Build: scan → filter → chunk → embed(batch) → upsert ANN → write meta → write manifest (tmp+rename) → verify (optional).
2) Query: embed(query) → ANN search → fetch metadata → format.
3) Chat Hook: pre-model call, run retrieval with token budget guard; include last 1–2 turns optionally.

---

## 3) Dependencies
- `fastembed` (Rust): local embeddings (ONNX) — pin to version compatible with MSRV 1.89.
- `hnsw_rs`: ANN index — cosine/IP support, persistence APIs.
- `tree-sitter` + `tree-sitter-rust`: code‑aware chunking for Rust.
- `ignore`: .gitignore traversal.
- `sha2`, `hex`: checksums.
- `serde`, `serde_json`, `anyhow`, `thiserror`.
- `time`/`chrono` for timestamps.

Notes
- Model cache location: XDG cache `~/.cache/codex-agentic/models`.
- Respect MSRV (1.89) and upstream workflows.

---

## 4) On-Disk Layout
```
.codex/
  index/
    manifest.json          # schema v1
    vectors.hnsw           # ANN graph + vectors
    meta.jsonl             # one JSON per chunk
    analytics.json         # {queries,hits,misses,last_query_ts}
    build.log              # optional
    lock                   # build lock file
```

### manifest.json (v1)
```json
{
  "index_version": 1,
  "engine": "fastembed",
  "model": "bge-small-en-v1.5",
  "dim": 384,
  "metric": "cosine",
  "chunk_mode": "auto", // auto=tree-sitter when available
  "chunk": {"lines": 160, "overlap": 32},
  "repo": {"root": ".", "git_sha": "<HEAD>"},
  "counts": {"files": 0, "chunks": 0},
  "checksums": {"vectors_hnsw": "<sha256>", "meta_jsonl": "<sha256>"},
  "created_at": "ISO-8601",
  "last_refresh": "ISO-8601"
}
```

### meta.jsonl (per line)
```json
{"id":12345,"path":"src/lib.rs","start":101,"end":180,"lang":"rust","sha256":"…","preview":"…"}
```

### analytics.json
```json
{"queries":123,"hits":97,"misses":26,"last_query_ts":"ISO-8601"}
```

---

## 5) CLI Design (clap)
```text
codex-agentic index
  build [--model bge-small|bge-large] [--force] [--chunk auto|lines] [--lines N] [--overlap M]
  query <text> [-k N] [--show-snippets]
  status
  verify
  clean
```

- Default model: `bge-small` (384‑D). Optional `bge-large` (1024‑D).
- Default chunk: `auto` (tree-sitter when available; else blank-lines; else lines).
- Exit codes: 0 success, >0 on failure (non-fatal to overall app startup).
- Env: `CODEX_INDEXING=0` disables automatic first-run build.

---

## 6) ACP/TUI Integration
- Slash commands mirror CLI: `/index build|query|status|verify|clean` and alias `/search <text>`.
- `/status` panel shows: state, last indexed (relative), model/dim, counts, size, analytics (queries + hit ratio).
- Retrieval hook: executed before model call; K=8 default; token cap for snippets; optional include last 1–2 turns.
- Post-turn background refresh: After each assistant response, if `now - last_check > N mins`, run git-delta refresh capped to M files/chunks; non-git repos skip.

---

## 7) Chunking Details
- Tree-sitter (Rust): produce nodes by function/impl/module; merge small nodes to target ~120–200 lines; add 20–40 line overlaps at boundaries.
- Blank-line fallback: split on >=2 consecutive blank lines; then pad to target window; enforce overlap.
- Lines fallback: fixed windows when above two fail.

Language Coverage (MVP)
- Rust grammar included. Others fall back. Future: add TypeScript/Python grammars.

---

## 8) Persistence & Integrity
- Build to temporary files: `vectors.hnsw.tmp`, `meta.jsonl.tmp`.
- Compute SHA256 for both; write to manifest; atomic `rename` to final paths.
- `lock` file prevents concurrent builds.
- `index verify`: checks manifest vs files, checksums, basic ANN integrity.

---

## 9) Analytics
- Update counters on each query:
  - `queries += 1`
  - `hits += (top1_score >= threshold ? 1 : 0)` (configurable, default 0.25)
  - `misses = queries - hits`
  - `last_query_ts = now`
- Write to `analytics.json.tmp` then `rename`.
- Display in `status` and `/status`.

---

## 10) Performance Targets
- Build: 5–8 min for ~20k chunks (384‑D) on typical laptop; batch size tuned (e.g., 64–128).
- Query: < 100 ms median (20k vectors, k=8) on CPU.
- Post-turn delta: ≤ 150 ms for scan + ≤ 1k chunks updated per run.

---

## 11) Error Handling
- Disk full/permission: surface concise errors; leave prior index intact.
- Model load failure: suggest `index clean` or switching model; continue without retrieval.
- Corruption: `index verify` fails → prompt to `index clean` or rebuild.

---

## 12) Testing Strategy
- Unit: chunkers, binary detection, manifest I/O, analytics atomicity.
- Integration: build → query loop on a sample repo; verify checksums.
- Performance smoke: measure batch embed + ANN search on synthetic corpus.

---

## 13) Rollout Plan
- Phase 1: Library + CLI (build/query/status/verify/clean), Rust tree-sitter, fallbacks, analytics.
- Phase 2: ACP/TUI commands + retrieval hook + `/status` integration.
- Phase 3: Post-turn git-delta refresh; non-git repos manual.
- Phase 4: Optional larger model flag; add more grammars.

---

## 14) Atomic Task Checklist

Phase 1 — Core + CLI
- [x] Add `codex_index` module (library in `codex-agentic` for MVP)
- [x] Add dependencies: fastembed, hnsw_rs, tree-sitter, tree-sitter-rust, ignore, serde, sha2
- [x] Implement `BinaryGuard::is_binary(path)` (magic bytes + UTF‑8 probe)
- [x] Implement `Scanner` using `ignore` crate
- [x] Implement `Chunker` (tree-sitter Rust → blank-line → lines)
- [x] Implement `Embedder` wrapper (model download/cache, batch API)
- [ ] Implement `IndexStore` (HNSW load/save, upsert, search)
- [x] Implement `MetaStore` (append + read by id)
- [x] Implement `Manifest` (load/save, checksums, compatibility)
- [x] Implement `Verifier` (checksums + structure)
- [x] Implement `AnalyticsStore` (atomic counters)
- [x] Implement CLI: `index build/query/status/verify/clean`
- [x] Wire default-on first run with opt-out env/flag
- [x] Add incremental git-delta rebuild (changed/untracked/deleted)

Phase 2 — ACP/TUI
- [x] Add `/index` and `/search` slash commands
- [x] Add retrieval hook before model call (ACP + TUI; simple char cap)
- [x] Extend `/status` panel to show index + analytics

Phase 3 — Background Refresh (timer-based MVP in place; git-delta full rebuild)
- [ ] Implement post-turn check + git-delta scan (timer-based delta implemented)
- [ ] Cap work per pass and add backoff
- [x] Update manifest `last_refresh` after deltas

Phase 4 — Enhancements
- [x] Add `--model bge-large` (1024‑D) path
- [ ] Add second grammar (TypeScript or Python)
- [ ] Add query-context option (include last 1–2 turns)

---

## Summary
- Implements local, default-on indexing with safe persistence, verification, and analytics.
- Delivers immediate value via CLI, with a clear path to ACP/TUI integration and incremental refresh.
- Keeps scope tight and portable; future-proofed with manifest versioning and modular components.


### Carry‑Over Notes (as of 0.39.0‑apc.7)
- ACP retrieval is in‑process (FastEmbed OnceLock + mmap + rayon). Display/injection is gated by the top confidence (CI):
  - Default threshold: `0.725` (override via `CODEX_INDEX_RETRIEVAL_THRESHOLD`).
  - If `top < threshold`: do not show anything in chat and do not inject context into the LLM.
  - If `top ≥ threshold`: inject context and show a compact summary line only: `> [CF%] -- # items found` (CF = `round(top*100)`).
  - Touchpoints:
    - Retrieval + refs: `codex-acp/src/agent.rs` (fetch + format in‑process)
    - Injection call site: `codex-acp/src/agent.rs` (before building `items`)
  - TUI mirrors ACP gating/summary: when `top ≥ threshold`, it renders the single summary line above and a spacer; otherwise it shows nothing. Implementation: `codex-tui/src/chatwidget.rs` (`submit_user_message` + `fetch_retrieval_context_plus`).
- Incremental indexing (git‑delta) is implemented in `codex-agentic/src/indexing/mod.rs` and currently performs rebuild‑and‑swap of flat vectors/meta. Background refresh is timer‑based (5 min).
- Version installed: `codex-agentic 0.39.0‑apc.7`.
- Env toggles:
  - `CODEX_INDEXING=0` disables auto build/refresh
- `CODEX_INDEX_RETRIEVAL=0` disables retrieval injection (ACP + TUI)
- `CODEX_INDEX_RETRIEVAL_THRESHOLD` adjusts the CI threshold (0.0–1.0; default `0.725`)

### Atomic Task Checklist (continuation)

Phase 1 — Core + CLI
- [ ] Implement `IndexStore` using HNSW (hnsw_rs) with on‑disk persistence
  - [ ] Build graph during `index build` (normalize vectors first)
  - [ ] Persist (bincode) + compaction/repair (`index doctor`)
  - [ ] CLI query prefers HNSW (fallback to mmap scan)

Phase 2 — ACP/TUI
- [x] ACP: in‑process retrieval (FastEmbed + mmap + rayon)
- [x] ACP: compact summary + thresholded context injection
  - [ ] TUI: in‑process retrieval (FastEmbed + mmap + rayon)
  - [x] TUI: compact summary before LLM; add spacer line
- [ ] Add CLI `index query --json` output for richer UI rendering

Phase 3 — Refresh & Scheduling
- [ ] Replace 5‑min timer with post‑turn refresh trigger (git‑delta)
- [ ] Cap work per pass (e.g., ≤1k chunks) and add backoff
- [x] Update manifest `last_refresh` after deltas

Phase 4 — Enhancements
- [ ] Token‑aware context budget using active model’s token limits
- [ ] Add second tree‑sitter grammar (TypeScript or Python) + fallback heuristics polish
- [ ] Add small LRU cache for recent queries (avoid re‑scoring repeats)
- [ ] Windows/symlink policy + long‑path handling
- [ ] Unit tests (binary detection, chunkers, manifest I/O, analytics) + integration tests (build→query)
- [ ] READMEs: user flags/env + `/status` fields + troubleshooting

### Resume Pointers
- HNSW integration entry points:
  - Build: `codex-agentic/src/indexing/mod.rs` inside `fn build` after batch embed
  - Query: same module’s `fn query` (add feature‑gated HNSW path)
- TUI retrieval port:
  - Injection point: `codex-tui/src/chatwidget.rs:~1218` (`submit_user_message`)
  - Shared helpers can live in a small internal module or duplicated minimal code (avoid new crate).
- ACP references style:
  - The summary is plain markdown; if the client supports HTML, we can switch back to `<details><summary>` later.
