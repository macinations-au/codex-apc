## Codex Agentic — Codebase Indexing PRD (Local‑Only)

### Status
- Draft → MVP scope ready for implementation
- Owners: codex-agentic maintainers
- Target: first usable POC behind default‑on local indexing

---

## 1) Why
- Improve answer quality by retrieving relevant in‑repo code/snippets before LLM generation.
- Fully offline, minimal footprint, no extra services or providers required.
- Fast to ship: simple CLI, background refresh, and ACP/TUI glue that injects top‑K matches into prompts.

Non‑Goals (MVP)
- No external embedding providers (OpenAI/Gemini/etc.).
- No vector database service by default (Qdrant optional later).
- No complex language parsers; chunking starts simple and code‑aware later.

---

## 2) Options Considered
- Local embeddings + in‑process ANN (HNSW) [Chosen].
  - Pros: zero services, small disk footprint, fast CPU search.
  - Cons: fewer ops features than a DB, manual persistence.
- Local embeddings + Qdrant (single Docker container) [Later/Optional].
  - Pros: durability, filters, REST/gRPC, tooling.
  - Cons: runs another service; out of MVP scope.
- File watchers vs periodic/idle scans
  - MVP: idle scans + on‑demand rebuild; cross‑platform, low complexity.
  - Later: file watchers where stable (macOS/Linux), fallback to polling on Windows.

---

## 3) MVP Requirements

Functional
- Default‑on local indexing for the current repo on first run (opt‑out flag).
- CLI: build/rebuild, query, status, clean.
- ACP/TUI: slash commands mirror CLI; retrieval auto‑hooks into chat turns.
- Background refresh: incremental, triggered after idle period or via manual command.
- Accurate ignore rules: respect `.gitignore`/`.ignore`, common build dirs, and binary detection via magic‑bytes.

Non‑Functional
- Offline‑only; no network calls.
- Low footprint: CPU‑only embeddings; modest RAM and disk.
- Fast: query top‑K under ~100 ms at 20–30k chunks on typical dev laptops.
- Robustness: crash‑safe manifest and index writes; atomic file replace.

Security/Privacy
- No code leaves the machine; embeddings and metadata stored locally under the repo.

User Experience
- Index builds automatically but stays unobtrusive; progress only when invoked, errors surfaced succinctly.
- `/status` shows index health: last indexed (relative), size, vectors, model.

---

## 4) Default Architecture

### Components
- Embedding Engine: `fastembed` (CPU, ONNX). Default model: `bge-small` (≈384‑D). Optional larger: `bge-large` (≈1024‑D).
- Chunker: line‑window based (e.g., 120–200 lines, 20–40 line overlap). Later: token‑aware + language heuristics.
- Index Store: HNSW (cosine/inner‑product), persisted to disk.
- Metadata Store: JSONL with file path, line range, language, sha256, and short preview.
- Refresh Service: background task that computes deltas and updates the index incrementally.
- Retriever: embeds queries, runs ANN search, returns top‑K with scores.

### Data Flow
1) Build: scan → filter/ignore → chunk → embed(batch) → upsert to HNSW → write metadata → write manifest.
2) Query: embed(query) → HNSW top‑K → read metadata → format snippets.
3) Chat Hook: before model call, run retrieval; inject results as a context block.

### On‑Disk Layout (default)
```
.codex/
  index/
    manifest.json          # model, dim, created_at, version, chunker config, repo id (git), counts
    vectors.hnsw           # serialized HNSW graph + vectors
    meta.jsonl             # one JSON per chunk: {id, path, start, end, lang, sha256, preview}
    build.log              # optional recent build log (rotated)
    lock                   # build lock to prevent concurrent writers
```

### Manifest (example)
```json
{
  "version": 1,
  "engine": "fastembed",
  "model": "bge-small-en-v1.5",
  "dim": 384,
  "metric": "cosine",
  "chunk": {"lines": 160, "overlap": 32},
  "repo": {"root": ".", "git_sha": "<HEAD>"},
  "counts": {"files": 812, "chunks": 21874},
  "created_at": "2025-09-24T08:00:00Z",
  "last_refresh": "2025-09-24T08:00:00Z"
}
```

---

## 5) CLI & Slash Commands

CLI (new `index` subcommands)
```bash
# Build or refresh (default model = small)
codex-agentic index build [--model bge-small|bge-large] [--force]

# Query the index
codex-agentic index query "<text>" -k 8 --show-snippets

# Show status
codex-agentic index status

# Clean (remove on-disk index)
codex-agentic index clean
```

ACP/TUI (slash)
- `/index build [--model …] [--force]`
- `/index query <text> [-k N]`
- `/index status`
- Alias: `/search <text>` calls the same query path.

`/status` Panel Additions
- “Index: Ready | Building | Stale | Missing”
- “Last indexed: now / 3h ago / 2d ago …”
- “Model: bge-small (384‑D), Vectors: 21,874, Size: 85 MB”

---

## 6) Background Refresh & Deltas

Triggering (MVP)
- Idle timer: after N minutes without active chat input (e.g., 3–5 min), run a quick delta scan.
- Manual: `index build` without `--force` performs incremental refresh.

Change Detection
- Prefer Git when available: `git ls-files -m -o --exclude-standard` for modified/untracked.
- Fallback: mtime + size + sha256 of small header.

Incremental Update
- Re‑chunk changed files; delete stale chunk IDs; re‑embed and upsert.
- Update manifest `last_refresh` and counts atomically.

Throttle & Safety
- Backoff between runs; skip if a build is already in progress (lock file).
- Cap work per cycle to keep UI responsive (e.g., 1k chunks/batch).

---

## 7) File Filtering & Binary Detection

Ignores
- Respect `.gitignore` / `.ignore` and built‑in denylist: `target/`, `node_modules/`, `.git/`, `.idea/`, `.vscode/`, `dist/`, large logs.
- Size caps (configurable): skip files > N MB (e.g., 2–5 MB by default).

Binary/Magic‑Bytes Check
- Read the first 4–8 KB; treat as binary if UTF‑8 fails and common magic signatures match (ELF, Mach‑O, PE, PNG/JPEG, PDF, ZIP/JAR, SQLite, etc.).

Fast Reads
- Use native Rust buffered reads; stream lines; avoid loading entire files.

---

## 8) Retrieval Injection

When enabled, before sending a user turn to the model:
- Embed the user query → ANN `top_k` (default k=8) → attach snippets.
- Formatting: a single “Context” section with file:line anchors and short previews.
- Token budget guard: truncate combined snippets to a configurable cap.

---

## 9) Performance Targets (MVP)
- Build (fresh, 5k files/20k chunks, 384‑D, CPU‑only): < 5–8 min on typical laptop.
- Query latency (20k vectors, k=8): < 100 ms median.
- RAM during build: < 1.5 GB; steady‑state query: < 200 MB.
- Disk: ≈ vectors × dims × 4B + graph overhead (e.g., 20k×384×4 ≈ 30 MB + HNSW).

---

## 10) Configuration

Runtime Flags
- `--indexing=on|off` (default: on)
- `--index-model=bge-small|bge-large` (default: bge-small)
- `--index-chunk-lines=160` `--index-chunk-overlap=32`
- `--index-k=8` (default retrieval K)
- `--index-idle-mins=5` (refresh trigger)
- `--index-skip-dirs=…` `--index-max-file-mb=5`

Environment (optional)
- `CODEX_INDEXING=0` to disable by default.

Manifest Compatibility
- If engine/model/dim mismatch, prompt to `--force` rebuild; otherwise perform incremental.

---

## 11) Telemetry & UX
- Console/TUI progress bars only on explicit builds.
- Concise errors with suggested fixes (e.g., “index missing → run index build”).
- Log file under `.codex/index/build.log` (rotated, last N runs).

---

## 12) Implementation Phases
1. Core library: fastembed wrapper, chunking, HNSW persistence, manifest.
2. CLI: `index build/query/status/clean`.
3. ACP/TUI: slash commands + retrieval hook + `/status` panel.
4. Background refresh (idle trigger + git deltas).
5. Larger models toggle and heuristic chunking improvements.

---

## 13) Open Questions
- Idle trigger default (3 vs 5 minutes)?
- Git submodules: index or skip by default?
- How to show partial index during initial build (progressive enable)?
- Windows path handling and long path limits.
- Future: pluggable filters (language, path globs) and reranking step.

---

## 14) Recommendations
- Ship MVP with `bge-small` (384‑D) as default; allow `bge-large` (1024‑D) via flag.
- Keep indexing on by default; first build starts in background with a polite notice.
- Retrieval injection default on; provide a toggle.
- Favor idle incremental refresh over watchers for portability; revisit watchers later.

---

## Appendix A — Pseudocode
```rust
fn build_index(root: Path) -> Result<()> {
    let cfg = load_cfg();
    let mut idx = Hnsw::open_or_new(root.join(".codex/index"))?;
    let files = scan_repo(root, &cfg.ignores)?;
    for file in files {
        if is_binary(&file) || too_big(&file) { continue; }
        for (span, text) in chunk_file(&file, cfg.chunk) {
            let emb = embedder.embed(&text)?; // fastembed
            idx.upsert(span.id, &emb)?;
            meta.write(span.id, meta_of(&file, span))?;
        }
    }
    manifest.update(...).write_atomic()?;
    Ok(())
}

fn query_index(q: &str, k: usize) -> Vec<Hit> {
    let qv = embedder.embed(q)?;
    idx.search(&qv, k).map(|id| meta.read(id))
}
```

---

## Summary
- Goal: fully local, default‑on code indexing to power retrieval in chat.
- Approach: `fastembed` + HNSW, simple chunking, background idle refresh, strong ignore/binary filters.
- Deliverables: CLI + ACP/TUI commands, status surfaced in `/status`, manifest‑driven persistence.

---

## Addendum — Decisions (2025-09-24)

- Default Behavior: Indexing is ON by default on first run; users can opt out (`--indexing=off` or `CODEX_INDEXING=0`). Index failures never block normal operations; the app continues regardless.
- Chunking (Default): `tree-sitter` is the default chunker. MVP ships the Rust grammar first; for other files, fall back to blank-line segmentation, then line-window as last resort. Manifest field `chunk_mode: "auto"` (auto = tree-sitter when available).
- Background Refresh Trigger: Replace generic idle timer with a predictable post-turn check. After each assistant response, if the last check > N minutes, run a capped git-delta refresh (non-git repos: manual rebuild only in MVP).
- Verification: Add `codex-agentic index verify` to validate index integrity (checksums, manifest compatibility). Persistence uses temp files + atomic rename. Periodic lightweight verification can run after successful builds.
- Analytics (Atomic): Add `.codex/index/analytics.json` with counters written atomically (tmp + rename): `{ queries, hits, misses, last_query_ts }`. Expose hit ratio and totals in `index status` and `/status`.
- Status Panel: Show State (Ready/Building/Stale/Missing), Last Indexed (relative), Model + Dim, Vectors/Files, Size on disk, and Analytics (queries, hit ratio).
- Storage Layout Update: Add `analytics.json` in the index directory.
- CLI Updates: Add `index verify`. Keep `index build/query/status/clean` as previously defined.

