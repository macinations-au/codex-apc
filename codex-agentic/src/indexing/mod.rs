use anyhow::{Context, Result, bail};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

// HNSW (approximate nearest neighbor) for fast query path
use hnsw_rs::prelude::*;

const INDEX_DIR: &str = ".codex/index";
const IGNORE_FILE: &str = ".index-ignore";
const VECTORS_FILE: &str = "vectors.hnsw"; // flat vectors + ids (linear scan + id map for HNSW)
const HNSW_BASENAME: &str = "vectors"; // will create vectors.hnsw.graph + vectors.hnsw.data // flat store for now
const META_FILE: &str = "meta.jsonl";
const MANIFEST_FILE: &str = "manifest.json";
const ANALYTICS_FILE: &str = "analytics.json";
const LOCK_FILE: &str = "lock";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    index_version: u32,
    engine: String,
    model: String,
    dim: usize,
    metric: String,
    chunk_mode: String,
    chunk: ChunkCfg,
    repo: RepoInfo,
    counts: Counts,
    checksums: Checksums,
    created_at: String,
    last_refresh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkCfg {
    lines: usize,
    overlap: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RepoInfo {
    root: String,
    git_sha: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Counts {
    files: usize,
    chunks: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Checksums {
    vectors_hnsw: String,
    meta_jsonl: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetaRow {
    id: u64,
    path: String,
    start: usize,
    end: usize,
    lang: String,
    sha256: String,
    preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Analytics {
    queries: u64,
    hits: u64,
    misses: u64,
    last_query_ts: Option<String>,
    // New: last time a background or manual index attempt/check ran
    last_attempt_ts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorStore {
    dim: usize,
    ids: Vec<u64>,
    data: Vec<f32>, // flattened row-major
}

impl VectorStore {
    fn new(dim: usize) -> Self {
        Self {
            dim,
            ids: Vec::new(),
            data: Vec::new(),
        }
    }
    fn push(&mut self, id: u64, v: &[f32]) {
        self.ids.push(id);
        self.data.extend_from_slice(v);
    }
    fn rows(&self) -> usize {
        self.ids.len()
    }
}

pub fn dispatch(cmd: crate::IndexCmd) -> Result<()> {
    match cmd {
        crate::IndexCmd::Build(args) => build(&args),
        crate::IndexCmd::Query(args) => query(&args),
        crate::IndexCmd::Status => status(),
        crate::IndexCmd::Verify => verify(),
        crate::IndexCmd::Clean => clean(),
        crate::IndexCmd::Ignore(args) => ignore_cmd(&args),
    }
}

// Public background helpers -------------------------------------------------
static STARTED_MAINTENANCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Spawn a best-effort first-run build in a background thread.
pub fn spawn_first_run_if_enabled() {
    if std::env::var("CODEX_INDEXING")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("off"))
        .unwrap_or(false)
    {
        return;
    }
    let manifest_exists = idx_dir().join(MANIFEST_FILE).exists();
    if manifest_exists {
        return;
    }
    std::thread::spawn(|| {
        let _ = build(&crate::IndexBuildArgs {
            model: "bge-small".into(),
            force: false,
            chunk: "auto".into(),
            lines: 160,
            overlap: 32,
        });
    });
}

/// Spawn periodic maintenance (simple timer-based) that triggers a rebuild
/// when git reports modified/untracked files. No-op if already started.
pub fn spawn_periodic_maintenance() {
    if STARTED_MAINTENANCE.set(()).is_err() {
        return;
    }
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(300)); // 5 minutes
            // Record an attempt timestamp regardless of whether we rebuild.
            let _ = update_analytics(|mut a| {
                a.last_attempt_ts = Some(now_iso());
                a
            });
            let changed = git_has_changes();
            if changed {
                let _ = build(&crate::IndexBuildArgs {
                    model: "bge-small".into(),
                    force: false,
                    chunk: "auto".into(),
                    lines: 160,
                    overlap: 32,
                });
            }
        }
    });
}

fn repo_root() -> PathBuf {
    if let Ok(out) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return PathBuf::from(s);
            }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn idx_dir() -> PathBuf {
    repo_root().join(INDEX_DIR)
}

fn ignore_file() -> PathBuf {
    repo_root().join(IGNORE_FILE)
}

const DEFAULT_IGNORE: &str = r"# Patterns ignored by the local code index (glob-like)
# Hidden files/dirs
.*
# VCS / tooling / caches
.git
.codex
.idea
.vscode
node_modules
target
dist
build
";

fn load_ignore_patterns() -> Vec<String> {
    let p = ignore_file();
    if !p.exists() {
        let _ = std::fs::write(&p, DEFAULT_IGNORE);
    }
    std::fs::read_to_string(&p)
        .map(|s| {
            s.lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(|l| l.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn save_ignore_patterns(mut pats: Vec<String>) -> anyhow::Result<()> {
    pats.sort();
    pats.dedup();
    let body = pats.join(
        "
",
    );
    std::fs::write(
        ignore_file(),
        format!(
            "{}
",
            body
        ),
    )
    .context("write .index-ignore")
}

fn glob_to_regex(glob: &str) -> Regex {
    let mut r = String::from("^");
    for ch in glob.chars() {
        match ch {
            '*' => r.push_str(".*"),
            '?' => r.push('.'),
            '.' | '+' | '(' | ')' | '|' | '{' | '}' | '[' | ']' | '^' | '$' | '\\' => {
                r.push('\\');
                r.push(ch);
            }
            _ => r.push(ch),
        }
    }
    r.push('$');
    Regex::new(&r).unwrap_or_else(|_| Regex::new(r"^$\b").unwrap())
}

fn ensure_dir() -> Result<()> {
    fs::create_dir_all(idx_dir()).context("create index dir")
}

fn now_iso() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".into())
}

fn read_git_head_sha() -> Option<String> {
    // Best-effort call to git if available
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn build(args: &crate::IndexBuildArgs) -> Result<()> {
    ensure_dir()?;
    let _guard = match BuildLock::acquire() {
        Ok(g) => Some(g),
        Err(_) => None, // if concurrent, become a no-op quietly
    };

    // Record an attempt timestamp (manual or scheduled)
    let _ = update_analytics(|mut a| {
        a.last_attempt_ts = Some(now_iso());
        a
    });

    // Scan repository files (apply .index-ignore + sanity limits)
    let root = repo_root();
    let mut files: Vec<PathBuf> = Vec::new();
    for dent in WalkBuilder::new(&root)
        .hidden(true)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .max_depth(None)
        .build()
    {
        let dent = match dent {
            Ok(d) => d,
            Err(_) => continue,
        };
        let path = dent.path();
        if !path.is_file() {
            continue;
        }
        if should_skip(path) {
            continue;
        }
        files.push(path.to_path_buf());
    }

    // Prepare embedding model
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    let model = match args.model.as_str() {
        "bge-large" => EmbeddingModel::BGELargeENV15,
        _ => EmbeddingModel::BGESmallENV15,
    };
    let opts = InitOptions::new(model);
    let mut embedder = TextEmbedding::try_new(opts).context("init fastembed")?;

    // Accumulate chunks, vectors (normalized), ids, and meta rows
    let mut all_vecs: Vec<Vec<f32>> = Vec::new();
    let mut all_ids: Vec<u64> = Vec::new();
    let mut meta_rows: Vec<MetaRow> = Vec::new();
    let mut next_id: u64 = 0;
    let mut file_count: usize = 0;

    for p in files.into_iter() {
        let text = match read_text_if_textual(&p)? {
            Some(t) => t,
            None => continue,
        };
        file_count += 1;
        let lang = language_for(&p);
        let chunks = match args.chunk.as_str() {
            "lines" => chunk_lines(&text, args.lines.max(8), args.overlap.min(args.lines / 2)),
            _ => chunk_auto(
                &text,
                &p,
                args.lines.max(80),
                args.overlap.min(args.lines / 2),
                &lang,
            ),
        };
        let relp = rel(&root, &p);
        for (s, e, prev) in chunks {
            let chunk = &text[s..e];
            // Embed (single item)
            let v = embedder
                .embed(vec![chunk.to_string()], None)
                .context("embed chunk")?
                .into_iter()
                .next()
                .unwrap_or_default();
            if v.is_empty() {
                continue;
            }
            // Normalize vector (L2)
            let mut vnorm = v;
            let mut norm = 0f32;
            for x in &vnorm {
                norm += *x as f32 * *x as f32;
            }
            let norm = norm.sqrt();
            if norm == 0.0 {
                continue;
            }
            for x in vnorm.iter_mut() {
                *x /= norm;
            }

            let id = next_id;
            next_id += 1;
            all_ids.push(id);
            all_vecs.push(vnorm);
            meta_rows.push(MetaRow {
                id,
                path: relp.clone(),
                start: offset_to_line(&text, s),
                end: offset_to_line(&text, e),
                lang: lang.clone(),
                sha256: sha256_hex(chunk),
                preview: prev,
            });
        }
    }

    if all_vecs.is_empty() {
        // Nothing to index; create minimal manifest and return Ok
        let created = now_iso();
        let m = Manifest {
            index_version: 1,
            engine: "fastembed+hnsw".into(),
            model: args.model.clone(),
            dim: 0,
            metric: "cosine".into(),
            chunk_mode: args.chunk.clone(),
            chunk: ChunkCfg {
                lines: args.lines,
                overlap: args.overlap,
            },
            repo: RepoInfo {
                root: root.to_string_lossy().into(),
                git_sha: read_git_head_sha(),
            },
            counts: Counts {
                files: file_count,
                chunks: 0,
            },
            checksums: Checksums {
                vectors_hnsw: String::new(),
                meta_jsonl: String::new(),
            },
            created_at: created.clone(),
            last_refresh: created,
        };
        fs::write(
            idx_dir().join(MANIFEST_FILE),
            serde_json::to_vec_pretty(&m)?,
        )?;
        return Ok(());
    }

    let dim = all_vecs[0].len();
    // Flatten vectors for our mmap-friendly linear scan file (ids + contiguous f32)
    let mut flat: Vec<f32> = Vec::with_capacity(dim * all_vecs.len());
    for v in &all_vecs {
        flat.extend_from_slice(&v[..]);
    }

    // Persist vectors + ids atomically
    let vec_tmp = idx_dir().join(format!("{}.tmp", VECTORS_FILE));
    write_vectors_file(&vec_tmp, dim as u32, &all_ids, &flat)?;
    fs::rename(&vec_tmp, idx_dir().join(VECTORS_FILE))?;

    // Persist meta.jsonl atomically
    let meta_tmp = idx_dir().join(format!("{}.tmp", META_FILE));
    {
        let mut w = BufWriter::new(File::create(&meta_tmp)?);
        for row in &meta_rows {
            let line = serde_json::to_string(row)?;
            w.write_all(line.as_bytes())?;
            w.write_all(
                b"
",
            )?;
        }
        w.flush()?;
    }
    fs::rename(&meta_tmp, idx_dir().join(META_FILE))?;

    // Build and persist HNSW graph (prefer fast query path)
    {
        let max_points = all_vecs.len();
        let m = 32usize; // max connections (M)
        let ef_c = 200usize; // construction ef
        let max_layer = 16usize;
        let dist = DistCosine::default();
        let hnsw: Hnsw<f32, DistCosine> = Hnsw::new(m, max_points, max_layer, ef_c, dist);
        // Prepare slice of (&vec, id)
        let items: Vec<(&[f32], usize)> = all_vecs
            .iter()
            .enumerate()
            .map(|(i, v)| (&v[..], i))
            .collect();
        hnsw.parallel_insert_slice(&items);
        // Dump alongside other index files: vectors.hnsw.graph + vectors.hnsw.data
        hnsw.file_dump(idx_dir().as_path(), HNSW_BASENAME)
            .context("dump HNSW files")?;
    }

    // Compute checksums for manifest
    let chk_vec = sha256_file(idx_dir().join(VECTORS_FILE))?;
    let chk_meta = sha256_file(idx_dir().join(META_FILE))?;

    let now = now_iso();
    let manifest = Manifest {
        index_version: 1,
        engine: "fastembed+hnsw".into(),
        model: args.model.clone(),
        dim,
        metric: "cosine".into(),
        chunk_mode: args.chunk.clone(),
        chunk: ChunkCfg {
            lines: args.lines,
            overlap: args.overlap,
        },
        repo: RepoInfo {
            root: root.to_string_lossy().into(),
            git_sha: read_git_head_sha(),
        },
        counts: Counts {
            files: file_count,
            chunks: all_ids.len(),
        },
        checksums: Checksums {
            vectors_hnsw: chk_vec,
            meta_jsonl: chk_meta,
        },
        created_at: now.clone(),
        last_refresh: now.clone(),
    };
    let man_tmp = idx_dir().join(format!("{}.tmp", MANIFEST_FILE));
    fs::write(&man_tmp, serde_json::to_vec_pretty(&manifest)?)?;
    fs::rename(&man_tmp, idx_dir().join(MANIFEST_FILE))?;

    Ok(())
}
fn verify() -> Result<()> {
    let m: Manifest = serde_json::from_slice(&fs::read(idx_dir().join(MANIFEST_FILE))?)?;
    let vchk = sha256_file(idx_dir().join(VECTORS_FILE))?;
    let mchk = sha256_file(idx_dir().join(META_FILE))?;
    if vchk == m.checksums.vectors_hnsw && mchk == m.checksums.meta_jsonl {
        println!("Index verify: OK");
        Ok(())
    } else {
        bail!("Index verify FAILED (checksums mismatch)")
    }
}

fn clean() -> Result<()> {
    if idx_dir().exists() {
        fs::remove_dir_all(idx_dir())?;
    }
    println!("Index removed");
    Ok(())
}

fn ignore_cmd(args: &crate::IndexIgnoreArgs) -> anyhow::Result<()> {
    if args.reset {
        std::fs::write(ignore_file(), DEFAULT_IGNORE).context("write default .index-ignore")?;
    }
    let mut pats = load_ignore_patterns();
    if !args.add.is_empty() {
        pats.extend(args.add.clone());
    }
    if !args.remove.is_empty() {
        let remove: std::collections::HashSet<String> = args.remove.iter().cloned().collect();
        pats.retain(|p| !remove.contains(p));
    }
    if args.reset || !args.add.is_empty() || !args.remove.is_empty() {
        save_ignore_patterns(pats)?;
    }
    if args.list || (!args.reset && args.add.is_empty() && args.remove.is_empty()) {
        print!(
            "Ignore file: {}
",
            ignore_file().display()
        );
        for p in load_ignore_patterns() {
            println!("{p}");
        }
    }
    Ok(())
}

// ---------- helpers ----------

fn load_vectors(path: impl AsRef<Path>) -> Result<(Vec<u64>, Vec<f32>)> {
    let mut f = BufReader::new(File::open(path)?);
    let mut buf4 = [0u8; 4];
    let mut buf8 = [0u8; 8];
    f.read_exact(&mut buf4)?; // magic
    f.read_exact(&mut buf4)?;
    let _dim = u32::from_le_bytes(buf4) as usize;
    f.read_exact(&mut buf8)?;
    let rows = u64::from_le_bytes(buf8) as usize;
    let mut ids = Vec::with_capacity(rows);
    for _ in 0..rows {
        f.read_exact(&mut buf8)?;
        ids.push(u64::from_le_bytes(buf8));
    }
    let mut data_bytes = Vec::new();
    f.read_to_end(&mut data_bytes)?;
    let n = rows.checked_mul(_dim).unwrap_or(0);
    let mut data = Vec::with_capacity(n);
    for i in 0..n {
        let off = i * 4;
        let v = f32::from_le_bytes([
            data_bytes[off],
            data_bytes[off + 1],
            data_bytes[off + 2],
            data_bytes[off + 3],
        ]);
        data.push(v);
    }
    Ok((ids, data))
}
fn write_vectors_file(path: impl AsRef<Path>, dim: u32, ids: &[u64], data: &[f32]) -> Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    // magic + dim + rows + ids + data
    w.write_all(b"VEC0")?;
    w.write_all(&dim.to_le_bytes())?;
    let rows = ids.len() as u64;
    w.write_all(&rows.to_le_bytes())?;
    for id in ids {
        w.write_all(&id.to_le_bytes())?;
    }
    // write f32 little endian
    for &v in data {
        w.write_all(&v.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}

fn load_meta(path: impl AsRef<Path>) -> Result<std::collections::HashMap<u64, MetaRow>> {
    let f = BufReader::new(File::open(path)?);
    let mut map = std::collections::HashMap::new();
    for line in f.lines() {
        let l = line?;
        if l.trim().is_empty() {
            continue;
        }
        let row: MetaRow = serde_json::from_str(&l)?;
        map.insert(row.id, row);
    }
    Ok(map)
}

fn sha256_file(path: impl AsRef<Path>) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn rel(root: &Path, p: &Path) -> String {
    pathdiff::diff_paths(p, root)
        .unwrap_or_else(|| p.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn language_for(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        _ => "text",
    }
    .into()
}

fn should_skip(path: &Path) -> bool {
    let relp = rel(&repo_root(), path);
    let patterns = load_ignore_patterns();
    if !patterns.is_empty() {
        for p in &patterns {
            let re = glob_to_regex(p);
            if re.is_match(&relp) {
                return true;
            }
        }
    }
    // Skip symlinks to avoid cycles and out-of-tree traversal
    if let Ok(md) = fs::symlink_metadata(path) {
        if md.file_type().is_symlink() {
            return true;
        }
    }
    // Conservative Windows path handling: skip very long paths
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wlen = path.as_os_str().encode_wide().count();
        if wlen > 245 { // ~MAX_PATH minus buffer
            return true;
        }
    }
    if let Ok(md) = fs::metadata(path) {
        if md.len() > 5 * 1024 * 1024 {
            return true;
        }
    }
    false
}

fn read_text_if_textual(path: &Path) -> Result<Option<String>> {
    let mut f = File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    if looks_binary(&buf) {
        return Ok(None);
    }
    Ok(String::from_utf8(buf).ok())
}

fn looks_binary(buf: &[u8]) -> bool {
    if buf.len() >= 4 {
        let magic = &buf[..8.min(buf.len())];
        const PDF: &[u8] = b"%PDF-";
        const ZIP: &[u8] = b"PK\x03\x04";
        const ELF: &[u8] = b"\x7FELF";
        const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
        const MZ: &[u8] = b"MZ";
        if magic.starts_with(PDF)
            || magic.starts_with(ZIP)
            || magic.starts_with(ELF)
            || magic.starts_with(PNG)
            || magic.starts_with(MZ)
        {
            return true;
        }
    }
    // if >10% NUL bytes or invalid UTF-8
    let nul = buf.iter().filter(|b| **b == 0).count();
    if nul * 10 > buf.len().max(1) {
        return true;
    }
    std::str::from_utf8(buf).is_err()
}

fn chunk_auto(
    text: &str,
    _path: &Path,
    target: usize,
    overlap: usize,
    lang: &str,
) -> Vec<(usize, usize, String)> {
    if lang == "rust" {
        if let Some(chunks) = chunk_rust_treesitter(text, target, overlap) {
            return chunks;
        }
    }
    if lang == "python" {
        if let Some(chunks) = chunk_python_treesitter(text, target, overlap) {
            return chunks;
        }
    }
    // fallback
    chunk_blanklines(text, target, overlap)
}

fn chunk_lines(text: &str, target: usize, overlap: usize) -> Vec<(usize, usize, String)> {
    // Build line start offsets
    let mut starts = vec![0usize];
    for (i, _) in text.match_indices("\n") {
        starts.push(i + 1);
    }
    let line_count = starts.len();
    let mut res = Vec::new();
    let mut cur = 1usize; // 1-based line index
    while cur <= line_count {
        let end_line = (cur + target - 1).min(line_count);
        let s = starts[cur - 1];
        let e = if end_line < line_count {
            starts[end_line]
        } else {
            text.len()
        };
        if e > s {
            res.push((s, e, preview(&text[s..e])));
        }
        if end_line == line_count {
            break;
        }
        let next = end_line.saturating_sub(overlap) + 1;
        if next <= cur {
            break;
        }
        cur = next;
    }
    res
}

fn chunk_blanklines(text: &str, target: usize, overlap: usize) -> Vec<(usize, usize, String)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\n' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            blocks.push((start, i + 1));
            start = i + 2;
            i += 2;
            continue;
        }
        i += 1;
    }
    blocks.push((start, text.len()));
    let mut res = Vec::new();
    let mut cur_start = blocks[0].0;
    let mut cur_end = blocks[0].1;
    for b in blocks.into_iter().skip(1) {
        if line_span(text, cur_start, b.1) < target {
            cur_end = b.1;
        } else {
            res.push((cur_start, cur_end, preview(&text[cur_start..cur_end])));
            cur_start = b.0;
            cur_end = b.1;
        }
    }
    res.push((cur_start, cur_end, preview(&text[cur_start..cur_end])));
    if overlap > 0 && res.len() > 1 {
        for i in 1..res.len() {
            let (s, e, pr) = res[i].clone();
            let new_s = back_n_lines(text, s, overlap);
            res[i] = (new_s, e, pr);
        }
    }
    res
}

fn back_n_lines(text: &str, mut off: usize, n: usize) -> usize {
    let mut k = 0;
    while off > 0 && k < n {
        off -= 1;
        if text.as_bytes()[off] == b'\n' {
            k += 1;
        }
    }
    off
}

fn line_span(text: &str, s: usize, e: usize) -> usize {
    text[s..e].bytes().filter(|b| *b == b'\n').count() + 1
}

fn offset_to_line(text: &str, off: usize) -> usize {
    text[..off].bytes().filter(|b| *b == b'\n').count() + 1
}

fn preview(chunk: &str) -> String {
    let mut p = chunk.lines().take(8).collect::<Vec<_>>().join("\n");
    if p.len() > 800 {
        p.truncate(800);
    }
    p
}

fn chunk_rust_treesitter(
    text: &str,
    target: usize,
    overlap: usize,
) -> Option<Vec<(usize, usize, String)>> {
    let mut parser = tree_sitter::Parser::new();

    let lang_ts: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&lang_ts).ok()?;
    let tree = parser.parse(text, None)?;
    let root = tree.root_node();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    fn collect(node: tree_sitter::Node, out: &mut Vec<(usize, usize)>) {
        let kind = node.kind();
        if matches!(
            kind,
            "function_item" | "impl_item" | "trait_item" | "struct_item" | "enum_item" | "mod_item"
        ) {
            let r = node.range();
            out.push((r.start_byte, r.end_byte));
        }
        for i in 0..node.child_count() {
            if let Some(ch) = node.child(i) {
                collect(ch, out);
            }
        }
    }
    collect(root, &mut ranges);
    if ranges.is_empty() {
        return None;
    }
    ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    let mut cur = ranges[0];
    for r in ranges.into_iter().skip(1) {
        if line_span(text, cur.0, r.1) < target {
            cur.1 = r.1;
        } else {
            merged.push(cur);
            cur = r;
        }
    }
    merged.push(cur);
    let mut res = Vec::new();
    for (s, e) in merged {
        res.push((s, e, preview(&text[s..e])));
    }
    if overlap > 0 && res.len() > 1 {
        for i in 1..res.len() {
            let (s, e, pr) = res[i].clone();
            let new_s = back_n_lines(text, s, overlap);
            res[i] = (new_s, e, pr);
        }
    }
    Some(res)
}


fn chunk_python_treesitter(
    text: &str,
    target: usize,
    overlap: usize,
) -> Option<Vec<(usize, usize, String)>> {
    let mut parser = tree_sitter::Parser::new();
    let lang_ts: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    if parser.set_language(&lang_ts).is_err() {
        return None;
    }
    let tree = parser.parse(text, None)?;
    let root = tree.root_node();
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    fn collect(node: tree_sitter::Node, out: &mut Vec<(usize, usize)>) {
        let kind = node.kind();
        if matches!(kind, "function_definition" | "class_definition") {
            let r = node.range();
            out.push((r.start_byte, r.end_byte));
        }
        for i in 0..node.child_count() {
            if let Some(ch) = node.child(i) {
                collect(ch, out);
            }
        }
    }
    collect(root, &mut ranges);
    if ranges.is_empty() {
        return None;
    }
    ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    let mut cur = ranges[0];
    for r in ranges.into_iter().skip(1) {
        if line_span(text, cur.0, r.1) < target {
            cur.1 = r.1;
        } else {
            merged.push(cur);
            cur = r;
        }
    }
    merged.push(cur);
    let mut res = Vec::new();
    for (s, e) in merged {
        res.push((s, e, preview(&text[s..e])));
    }
    if overlap > 0 && res.len() > 1 {
        for i in 1..res.len() {
            let (s, e, pr) = res[i].clone();
            let new_s = back_n_lines(text, s, overlap);
            res[i] = (new_s, e, pr);
        }
    }
    Some(res)
}

// Embedding: fastembed wrapper (sync)
fn embed_text(model_name: &str, text: &str) -> Result<Vec<f32>> {
    use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
    let model = match model_name {
        "bge-large-en-v1.5" => EmbeddingModel::BGELargeENV15,
        _ => EmbeddingModel::BGESmallENV15,
    };
    let opts = InitOptions::new(model);
    let mut embedder = TextEmbedding::try_new(opts).context("init fastembed")?;
    let out = embedder
        .embed(vec![text.to_string()], None)
        .context("embed")?;
    Ok(out.into_iter().next().unwrap_or_default())
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0f32;
    let mut na = 0f32;
    let mut nb = 0f32;
    for i in 0..a.len() {
        let x = a[i];
        let y = b[i];
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

// ---------- analytics ----------
fn read_analytics() -> Result<Analytics> {
    let p = idx_dir().join(ANALYTICS_FILE);
    if !p.exists() {
        return Ok(Analytics::default());
    }
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}

fn update_analytics<F: FnOnce(Analytics) -> Analytics>(f: F) -> Result<()> {
    let a0 = read_analytics().unwrap_or_default();
    let a1 = f(a0);
    let tmp = idx_dir().join(format!("{}.tmp", ANALYTICS_FILE));
    fs::write(&tmp, serde_json::to_vec_pretty(&a1)?)?;
    fs::rename(tmp, idx_dir().join(ANALYTICS_FILE))?;
    Ok(())
}

fn min_score_threshold() -> f32 {
    std::env::var("CODEX_INDEX_RETRIEVAL_THRESHOLD")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .map(|v| v.clamp(0.0, 1.0))
        .unwrap_or(0.60)
}

fn xml_escape(s: &str) -> String {
    s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace("\"", "&quot;")
        .replace("'", "&apos;")
}

fn query(args: &crate::IndexQueryArgs) -> anyhow::Result<()> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(idx_dir().join(MANIFEST_FILE))?)?;
    let (ids, data) = load_vectors(idx_dir().join(VECTORS_FILE))?;
    let meta = load_meta(idx_dir().join(META_FILE))?;

    // Embed + normalize query
    let mut qv = embed_text(&manifest.model, &args.query)?;
    if qv.len() != manifest.dim {
        bail!("query dim {} != index dim {}", qv.len(), manifest.dim);
    }
    let mut qn = 0f32;
    for x in &qv {
        qn += *x * *x;
    }
    let qn = qn.sqrt();
    if qn > 0.0 {
        for x in qv.iter_mut() {
            *x /= qn;
        }
    }

    // Prefer HNSW when files exist; fallback to linear scan
    let graph_path = idx_dir().join(format!("{}.hnsw.graph", HNSW_BASENAME));
    let data_path = idx_dir().join(format!("{}.hnsw.data", HNSW_BASENAME));
    let use_hnsw = graph_path.exists() && data_path.exists();

    let mut scores: Vec<(usize, f32)> = Vec::new();
    if use_hnsw {
        let mut reloader = HnswIo::new(idx_dir().as_path(), HNSW_BASENAME);
        let options = hnsw_rs::prelude::ReloadOptions::default().set_mmap(true);
        reloader.set_options(options);
        let hnsw: Hnsw<f32, DistCosine> = reloader.load_hnsw::<f32, DistCosine>()?;
        let ef_s = 256usize;
        let max_k = args.k.min(ids.len());
        let result = hnsw.search(&qv, max_k, ef_s);
        for n in result.iter() {
            // DistCosine distance in [0,2]; similarity ~ 1 - d
            let sim = 1.0f32 - (n.distance as f32);
            scores.push((n.d_id, sim)); // d_id is the insertion id (usize)
        }
    } else {
        let k = args.k.min(ids.len());
        scores = (0..ids.len())
            .map(|i| {
                (
                    i,
                    cosine(&qv, &data[i * manifest.dim..(i + 1) * manifest.dim]),
                )
            })
            .collect();
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(k);
    }

    // Sort scores (desc) to ensure top-first regardless of path
    scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Confidence gating: hide low-confidence results (< threshold)
    let threshold = min_score_threshold();
    let top = scores.first().map(|(_, s)| *s).unwrap_or(0.0);
    if scores.is_empty() || top < threshold {
        match args.output {
            crate::OutputFormatArg::Text => {
                println!("No information exists that matches the request.");
            }
            crate::OutputFormatArg::Json => {
                println!("[]");
            }
            crate::OutputFormatArg::Xml => {
                println!("<results/>");
            }
        }
        update_analytics(|mut a| {
            a.queries += 1;
            let hit = if top >= threshold { 1 } else { 0 };
            a.hits += hit;
            a.misses = a.queries.saturating_sub(a.hits);
            a.last_query_ts = Some(now_iso());
            a
        })?;
        return Ok(());
    }

    let mut out_items: Vec<serde_json::Value> = Vec::new();
    let k = args.k.min(scores.len());
    for (rank, (i, score)) in scores.iter().take(k).enumerate() {
        let id = ids[*i];
        if let Some(row) = meta.get(&id) {
            let snippet_lines = if args.show_snippets {
                Some(row.preview.lines().collect::<Vec<_>>())
            } else {
                None
            };
            out_items.push(serde_json::json!({
                "rank": rank,
                "score": (*score as f64),
                "path": row.path,
                "start": row.start,
                "end": row.end,
                "lang": row.lang,
                "snippet": snippet_lines,
            }));
        }
    }

    match args.output {
        crate::OutputFormatArg::Text => {
            for item in &out_items {
                let rank = item["rank"].as_u64().unwrap_or(0);
                let score = item["score"].as_f64().unwrap_or(0.0);
                let path = item["path"].as_str().unwrap_or("");
                let start = item["start"].as_u64().unwrap_or(0);
                let end = item["end"].as_u64().unwrap_or(0);
                let lang = item["lang"].as_str().unwrap_or("");
                println!("[{rank}] {score:.3} {path}:{start}-{end} ({lang})");
                if let Some(snippet) = item["snippet"].as_array() {
                    let start_num = start as usize;
                    for (i, line) in snippet.iter().filter_map(|v| v.as_str()).enumerate() {
                        let prefix = if args.diff { "+ " } else { "" };
                        if args.no_line_numbers {
                            println!("{prefix}{line}");
                        } else {
                            let ln = start_num.saturating_add(i);
                            println!(
                                "{ln:>width$} | {prefix}{line}",
                                width = args.line_number_width
                            );
                        }
                    }
                    println!("---");
                }
            }
        }
        crate::OutputFormatArg::Json => {
            println!("{}", serde_json::to_string_pretty(&out_items).unwrap());
        }
        crate::OutputFormatArg::Xml => {
            println!("<results>");
            for item in &out_items {
                let rank = item["rank"].as_u64().unwrap_or(0);
                let score = item["score"].as_f64().unwrap_or(0.0);
                let path = xml_escape(item["path"].as_str().unwrap_or(""));
                let start = item["start"].as_u64().unwrap_or(0);
                let end = item["end"].as_u64().unwrap_or(0);
                let lang = xml_escape(item["lang"].as_str().unwrap_or(""));
                println!(
                    r#"  <hit rank="{rank}" score="{score:.3}" path="{path}" start="{start}" end="{end}" lang="{lang}">"#
                );
                if let Some(snippet) = item["snippet"].as_array() {
                    println!("    <snippet>");
                    let start_num = start as usize;
                    for (i, line) in snippet.iter().filter_map(|v| v.as_str()).enumerate() {
                        let ln = start_num.saturating_add(i);
                        if args.no_line_numbers {
                            if args.diff {
                                println!(r#"      <line op="add">{}</line>"#, xml_escape(line));
                            } else {
                                println!(r#"      <line>{}</line>"#, xml_escape(line));
                            }
                        } else {
                            if args.diff {
                                println!(
                                    r#"      <line n="{ln}" op="add">{}</line>"#,
                                    xml_escape(line)
                                );
                            } else {
                                println!(r#"      <line n="{ln}">{}</line>"#, xml_escape(line));
                            }
                        }
                    }
                    println!(
                        "    </snippet>
  </hit>"
                    );
                } else {
                    println!("  </hit>");
                }
            }
            println!("</results>");
        }
    }
    update_analytics(|mut a| {
        a.queries += 1;
        let threshold = min_score_threshold();
        let hit = if scores.get(0).map(|(_, s)| *s >= threshold).unwrap_or(false) {
            1
        } else {
            0
        };
        a.hits += hit;
        a.misses = a.queries.saturating_sub(a.hits);
        a.last_query_ts = Some(now_iso());
        a
    })?;
    Ok(())
}
fn status() -> anyhow::Result<()> {
    let manifest_path = idx_dir().join(MANIFEST_FILE);
    if !manifest_path.exists() {
        println!("Index: Missing");
        return Ok(());
    }
    let m: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    let an: Analytics = read_analytics().unwrap_or_default();
    let vec_sz = fs::metadata(idx_dir().join(VECTORS_FILE))
        .map(|m| m.len())
        .unwrap_or(0);
    println!(
        "Index: Ready
Last indexed: {}
Model: {} ({}-D)
Vectors: {}
Size: {} bytes
Analytics: queries={}, hit_ratio={:.2}",
        relative_age(&m.last_refresh).unwrap_or_else(|| m.last_refresh.clone()),
        m.model,
        m.dim,
        m.counts.chunks,
        vec_sz,
        an.queries,
        if an.queries > 0 {
            an.hits as f32 / an.queries as f32
        } else {
            0.0
        }
    );
    Ok(())
}

// ---------- git & locking & time helpers ----------
fn git_has_changes() -> bool {
    fn git_changed_sets() -> (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    ) {
        use std::process::Command;
        let mut changed = std::collections::HashSet::new();
        let mut deleted = std::collections::HashSet::new();
        let root = repo_root();
        let run = |args: &[&str]| -> Option<String> {
            let out = Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .ok()?;
            if out.status.success() {
                Some(String::from_utf8_lossy(&out.stdout).to_string())
            } else {
                None
            }
        };
        if let Some(s) = run(&["ls-files", "-m"]) {
            for l in s.lines() {
                if !l.trim().is_empty() {
                    changed.insert(l.trim().to_string());
                }
            }
        }
        if let Some(s) = run(&["ls-files", "-o", "--exclude-standard"]) {
            for l in s.lines() {
                if !l.trim().is_empty() {
                    changed.insert(l.trim().to_string());
                }
            }
        }
        if let Some(s) = run(&["ls-files", "-d"]) {
            for l in s.lines() {
                if !l.trim().is_empty() {
                    deleted.insert(l.trim().to_string());
                }
            }
        }
        (changed, deleted)
    }

    let out = std::process::Command::new("git")
        .args(["ls-files", "-m", "-o", "--exclude-standard"])
        .output();
    if let Ok(o) = out {
        !o.stdout.is_empty()
    } else {
        false
    }
}

struct BuildLock(std::path::PathBuf);
impl BuildLock {
    fn acquire() -> Result<Self> {
        let p = idx_dir().join(LOCK_FILE);
        match std::fs::File::options()
            .create_new(true)
            .write(true)
            .open(&p)
        {
            Ok(_) => Ok(Self(p)),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    }
}
impl Drop for BuildLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn relative_age(iso: &str) -> Option<String> {
    let t =
        time::OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339).ok()?;
    let now = time::OffsetDateTime::now_utc();
    let diff = now - t;
    let secs = diff.whole_seconds();
    Some(if secs < 60 {
        "now".into()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else if secs < 86400 * 7 {
        format!("{}d ago", secs / 86400)
    } else {
        format!("{}w ago", secs / (86400 * 7))
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_binary_png() {
        let png: &[u8] = b"\x89PNG\r\n\x1a\nrest";
        assert!(looks_binary(png));
    }

    #[test]
    fn chunk_lines_overlap() {
        let text = (1..=120).map(|i| format!("line{}
", i)).collect::<String>();
        let chunks = chunk_lines(&text, 40, 10);
        assert!(!chunks.is_empty());
        // Ensure chunks overlap by ~10 lines
        if chunks.len() > 1 {
            let (s0, e0, _) = chunks[0].clone();
            let (s1, _e1, _) = chunks[1].clone();
            let l0_end = offset_to_line(&text, e0);
            let l1_start = offset_to_line(&text, s1);
            assert!(l1_start <= l0_end);
        }
    }

    #[test]
    fn xml_escape_basic() {
        let s = r#"<tag> & " ' ;"#;
        let e = xml_escape(s);
        assert!(e.contains("&lt;tag&gt;"));
        assert!(e.contains("&amp;"));
        assert!(e.contains("&quot;"));
        assert!(e.contains("&apos;"));
    }
}
