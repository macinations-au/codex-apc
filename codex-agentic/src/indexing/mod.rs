use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

const INDEX_DIR: &str = ".codex/index";
const VECTORS_FILE: &str = "vectors.hnsw"; // flat store for now
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VectorStore {
    dim: usize,
    ids: Vec<u64>,
    data: Vec<f32>, // flattened row-major
}

impl VectorStore {
    fn new(dim: usize) -> Self { Self { dim, ids: Vec::new(), data: Vec::new() } }
    fn push(&mut self, id: u64, v: &[f32]) { self.ids.push(id); self.data.extend_from_slice(v); }
    fn rows(&self) -> usize { self.ids.len() }
}

pub fn dispatch(cmd: crate::IndexCmd) -> Result<()> {
    match cmd {
        crate::IndexCmd::Build(args) => build(&args),
        crate::IndexCmd::Query(args) => query(&args),
        crate::IndexCmd::Status => status(),
        crate::IndexCmd::Verify => verify(),
        crate::IndexCmd::Clean => clean(),
    }
}

// Public background helpers -------------------------------------------------
static STARTED_MAINTENANCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Spawn a best-effort first-run build in a background thread.
pub fn spawn_first_run_if_enabled() {
    if std::env::var("CODEX_INDEXING").map(|v| v=="0" || v.eq_ignore_ascii_case("off")).unwrap_or(false) {
        return;
    }
    let manifest_exists = idx_dir().join(MANIFEST_FILE).exists();
    if manifest_exists { return; }
    std::thread::spawn(|| {
        let _ = build(&crate::IndexBuildArgs{ model: "bge-small".into(), force: false, chunk: "auto".into(), lines: 160, overlap: 32 });
    });
}

/// Spawn periodic maintenance (simple timer-based) that triggers a rebuild
/// when git reports modified/untracked files. No-op if already started.
pub fn spawn_periodic_maintenance() {
    if STARTED_MAINTENANCE.set(()).is_err() { return; }
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(300)); // 5 minutes
            let changed = git_has_changes();
            if changed {
                let _ = build(&crate::IndexBuildArgs{ model: "bge-small".into(), force: false, chunk: "auto".into(), lines: 160, overlap: 32 });
            }
        }
    });
}

fn repo_root() -> PathBuf {
    if let Ok(out) = std::process::Command::new("git").args(["rev-parse","--show-toplevel"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() { return PathBuf::from(s); }
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn idx_dir() -> PathBuf { repo_root().join(INDEX_DIR) }

fn ensure_dir() -> Result<()> { fs::create_dir_all(idx_dir()).context("create index dir") }

fn now_iso() -> String { OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "1970-01-01T00:00:00Z".into()) }

fn read_git_head_sha() -> Option<String> {
    // Best-effort call to git if available
    let out = std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else { None }
}

fn build(args: &crate::IndexBuildArgs) -> Result<()> {
    ensure_dir()?;
    let _guard = match BuildLock::acquire() { Ok(g) => Some(g), Err(_) => None };
    let root = std::env::current_dir()?;
    let manifest_path = idx_dir().join(MANIFEST_FILE);

    // Model selection
    let (model_name, dim) = match args.model.as_str() {
        "bge-large" => ("bge-large-en-v1.5", 1024usize),
        _ => ("bge-small-en-v1.5", 384usize),
    };

    // Scan files and produce chunks
    let mut files_indexed = 0usize;
    let mut chunks_indexed = 0usize;
    let mut vectors = VectorStore::new(dim);

    let meta_tmp = idx_dir().join(format!("{}.tmp", META_FILE));
    let mut meta_writer = BufWriter::new(File::create(&meta_tmp)?);

    let mut id_counter: u64 = 0;
    for entry in ignore::WalkBuilder::new(&root)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true)
        .build()
    {
        let entry = match entry { Ok(e) => e, Err(_) => continue };
        let path = entry.path();
        if path.is_dir() { continue; }
        if should_skip(path) { continue; }
        let lang = language_for(path);
        let text = match read_text_if_textual(path) { Ok(Some(t)) => t, _ => continue };
        files_indexed += 1;
        let chunks = match args.chunk.as_str() {
            "auto" => chunk_auto(&text, path, args.lines, args.overlap, &lang),
            _ => chunk_lines(&text, args.lines, args.overlap),
        };
        for (start, end, preview) in chunks {
            let emb = embed_text(model_name, &text[start..end])?;
            if emb.len() != dim { bail!("embedding dimension mismatch: got {}, want {}", emb.len(), dim); }
            let id = id_counter; id_counter += 1; chunks_indexed += 1;
            vectors.push(id, &emb);
            let sha = sha256_hex(&text[start..end]);
            let row = MetaRow { id, path: rel(&root, path), start: offset_to_line(&text, start), end: offset_to_line(&text, end), lang: lang.clone(), sha256: sha, preview };
            serde_json::to_writer(&mut meta_writer, &row)?; meta_writer.write_all(b"\n")?;
        }
    }
    drop(meta_writer);

    // Persist vectors flat
    let vectors_tmp = idx_dir().join(format!("{}.tmp", VECTORS_FILE));
    {
        let mut w = BufWriter::new(File::create(&vectors_tmp)?);
        // simple binary: [u32 magic][u32 dim][u64 rows] then ids then f32 data
        w.write_all(&0xC0D3u32.to_le_bytes())?;
        w.write_all(&(vectors.dim as u32).to_le_bytes())?;
        w.write_all(&(vectors.rows() as u64).to_le_bytes())?;
        for id in &vectors.ids { w.write_all(&id.to_le_bytes())?; }
        let bytes = bytemuck::cast_slice::<f32, u8>(&vectors.data);
        w.write_all(bytes)?;
        w.flush()?;
    }

    // Checksums
    let chk_vec = sha256_file(&vectors_tmp)?;
    let chk_meta = sha256_file(&meta_tmp)?;

    // Write manifest tmp
    let manifest = Manifest {
        index_version: 1,
        engine: "fastembed".into(),
        model: model_name.into(),
        dim,
        metric: "cosine".into(),
        chunk_mode: args.chunk.clone(),
        chunk: ChunkCfg { lines: args.lines, overlap: args.overlap },
        repo: RepoInfo { root: ".".into(), git_sha: read_git_head_sha() },
        counts: Counts { files: files_indexed, chunks: chunks_indexed },
        checksums: Checksums { vectors_hnsw: chk_vec.clone(), meta_jsonl: chk_meta.clone() },
        created_at: now_iso(),
        last_refresh: now_iso(),
    };
    let manifest_tmp = idx_dir().join(format!("{}.tmp", MANIFEST_FILE));
    fs::write(&manifest_tmp, serde_json::to_vec_pretty(&manifest)?)?;

    // Atomic renames
    fs::rename(vectors_tmp, idx_dir().join(VECTORS_FILE))?;
    fs::rename(meta_tmp, idx_dir().join(META_FILE))?;
    fs::rename(manifest_tmp, manifest_path)?;

    eprintln!("Index build complete: {} files, {} chunks", files_indexed, chunks_indexed);
    Ok(())
}

fn query(args: &crate::IndexQueryArgs) -> Result<()> {
    let manifest: Manifest = serde_json::from_slice(&fs::read(idx_dir().join(MANIFEST_FILE))?)?;
    let (ids, data) = load_vectors(idx_dir().join(VECTORS_FILE))?;
    let meta = load_meta(idx_dir().join(META_FILE))?;
    let qv = embed_text(&manifest.model, &args.query)?;
    if qv.len()!=manifest.dim { bail!("query dim {} != index dim {}", qv.len(), manifest.dim); }
    let k = args.k.min(ids.len());
    let mut scores: Vec<(usize, f32)> = (0..ids.len()).map(|i| (i, cosine(&qv, &data[i*manifest.dim..(i+1)*manifest.dim]))).collect();
    scores.sort_by(|a,b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut hits = 0u64;
    for (rank,(i,score)) in scores.iter().take(k).enumerate() {
        if *score >= 0.25 { hits += 1; }
        let id = ids[*i];
        if let Some(row) = meta.get(&id) {
            println!("[{rank}] {score:.3} {}:{}-{} ({})", row.path, row.start, row.end, row.lang);
            if args.show_snippets { println!("{}\n---", row.preview); }
        }
    }
    // analytics
    update_analytics(|mut a| { a.queries+=1; let hit = if scores.get(0).map(|(_,s)| *s >= 0.25).unwrap_or(false) {1} else {0}; a.hits+=hit; a.misses = a.queries.saturating_sub(a.hits); a.last_query_ts=Some(now_iso()); a })?;
    Ok(())
}

fn status() -> Result<()> {
    let manifest_path = idx_dir().join(MANIFEST_FILE);
    if !manifest_path.exists() { println!("Index: Missing"); return Ok(()); }
    let m: Manifest = serde_json::from_slice(&fs::read(&manifest_path)?)?;
    let an: Analytics = read_analytics().unwrap_or_default();
    let vec_sz = fs::metadata(idx_dir().join(VECTORS_FILE)).map(|m| m.len()).unwrap_or(0);
    println!(
        "Index: Ready\nLast indexed: {}\nModel: {} ({}-D)\nVectors: {}\nSize: {} bytes\nAnalytics: queries={}, hit_ratio={:.2}",
        relative_age(&m.last_refresh).unwrap_or_else(|| m.last_refresh.clone()), m.model, m.dim, m.counts.chunks, vec_sz, an.queries, if an.queries>0 { an.hits as f32 / an.queries as f32 } else { 0.0 }
    );
    Ok(())
}

fn verify() -> Result<()> {
    let m: Manifest = serde_json::from_slice(&fs::read(idx_dir().join(MANIFEST_FILE))?)?;
    let vchk = sha256_file(idx_dir().join(VECTORS_FILE))?;
    let mchk = sha256_file(idx_dir().join(META_FILE))?;
    if vchk==m.checksums.vectors_hnsw && mchk==m.checksums.meta_jsonl {
        println!("Index verify: OK");
        Ok(())
    } else {
        bail!("Index verify FAILED (checksums mismatch)")
    }
}

fn clean() -> Result<()> {
    if idx_dir().exists() { fs::remove_dir_all(idx_dir())?; }
    println!("Index removed"); Ok(())
}

// ---------- helpers ----------

fn load_vectors(path: impl AsRef<Path>) -> Result<(Vec<u64>, Vec<f32>)> {
    let mut f = BufReader::new(File::open(path)?);
    let mut buf4=[0u8;4]; let mut buf8=[0u8;8];
    f.read_exact(&mut buf4)?; // magic
    f.read_exact(&mut buf4)?; let _dim = u32::from_le_bytes(buf4) as usize;
    f.read_exact(&mut buf8)?; let rows = u64::from_le_bytes(buf8) as usize;
    let mut ids=Vec::with_capacity(rows);
    for _ in 0..rows { f.read_exact(&mut buf8)?; ids.push(u64::from_le_bytes(buf8)); }
    let mut data_bytes = Vec::new(); f.read_to_end(&mut data_bytes)?;
    let n = rows.checked_mul(_dim).unwrap_or(0);
    let mut data = Vec::with_capacity(n);
    for i in 0..n { let off=i*4; let v=f32::from_le_bytes([data_bytes[off],data_bytes[off+1],data_bytes[off+2],data_bytes[off+3]]); data.push(v); }
    Ok((ids, data))
}

fn load_meta(path: impl AsRef<Path>) -> Result<std::collections::HashMap<u64, MetaRow>> {
    let f = BufReader::new(File::open(path)?);
    let mut map = std::collections::HashMap::new();
    for line in f.lines() {
        let l = line?; if l.trim().is_empty() { continue; }
        let row: MetaRow = serde_json::from_str(&l)?; map.insert(row.id, row);
    }
    Ok(map)
}

fn sha256_file(path: impl AsRef<Path>) -> Result<String> {
    let mut f = File::open(path)?; let mut hasher = Sha256::new(); let mut buf=[0u8;8192];
    loop { let n=f.read(&mut buf)?; if n==0 { break; } hasher.update(&buf[..n]); }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_hex(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }

fn rel(root: &Path, p: &Path) -> String { pathdiff::diff_paths(p, root).unwrap_or_else(|| p.to_path_buf()).to_string_lossy().to_string() }

fn language_for(path: &Path) -> String {
    match path.extension().and_then(|s| s.to_str()).unwrap_or("") {
        "rs" => "rust",
        "ts"|"tsx" => "typescript",
        "js"|"jsx" => "javascript",
        "py" => "python",
        _ => "text",
    }.into()
}

fn should_skip(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
        if name.starts_with('.') { return true; }
    }
    let comps: Vec<_> = path.components().collect();
    let deny = [".git","target","node_modules","dist","build",".idea",".vscode"];
    if comps.iter().any(|c| deny.iter().any(|d| c.as_os_str().to_string_lossy()==*d)) { return true; }
    if let Ok(md)=fs::metadata(path) { if md.len() > 5*1024*1024 { return true; } }
    false
}

fn read_text_if_textual(path: &Path) -> Result<Option<String>> {
    let mut f = File::open(path)?; let mut buf = Vec::new(); f.read_to_end(&mut buf)?;
    if looks_binary(&buf) { return Ok(None); }
    Ok(String::from_utf8(buf).ok())
}

fn looks_binary(buf: &[u8]) -> bool {
    if buf.len() >= 4 {
        let magic = &buf[..8.min(buf.len())];
        const PDF:&[u8]=b"%PDF-"; const ZIP:&[u8]=b"PK\x03\x04"; const ELF:&[u8]=b"\x7FELF"; const PNG:&[u8]=b"\x89PNG\r\n\x1a\n"; const MZ:&[u8]=b"MZ";
        if magic.starts_with(PDF) || magic.starts_with(ZIP) || magic.starts_with(ELF) || magic.starts_with(PNG) || magic.starts_with(MZ) { return true; }
    }
    // if >10% NUL bytes or invalid UTF-8
    let nul = buf.iter().filter(|b| **b==0).count();
    if nul*10 > buf.len().max(1) { return true; }
    std::str::from_utf8(buf).is_err()
}

fn chunk_auto(text: &str, _path: &Path, target: usize, overlap: usize, lang: &str) -> Vec<(usize,usize,String)> {
    if lang=="rust" {
        if let Some(chunks) = chunk_rust_treesitter(text, target, overlap) { return chunks; }
    }
    // fallback
    chunk_blanklines(text, target, overlap)
}

fn chunk_lines(text: &str, target: usize, overlap: usize) -> Vec<(usize,usize,String)> {
    // Build line start offsets
    let mut starts = vec![0usize];
    for (i, _) in text.match_indices("\n") { starts.push(i+1); }
    let line_count = starts.len();
    let mut res = Vec::new();
    let mut cur = 1usize; // 1-based line index
    while cur <= line_count {
        let end_line = (cur + target - 1).min(line_count);
        let s = starts[cur-1];
        let e = if end_line < line_count { starts[end_line] } else { text.len() };
        if e > s { res.push((s, e, preview(&text[s..e]))); }
        if end_line == line_count { break; }
        let next = end_line.saturating_sub(overlap) + 1;
        if next <= cur { break; }
        cur = next;
    }
    res
}


fn chunk_blanklines(text: &str, target: usize, overlap: usize) -> Vec<(usize,usize,String)> {
    let mut blocks: Vec<(usize,usize)> = Vec::new();
    let mut start=0usize; let mut i=0usize; let bytes=text.as_bytes();
    while i < bytes.len() {
        if bytes[i]==b'\n' && i+1 < bytes.len() && bytes[i+1]==b'\n' {
            blocks.push((start, i+1)); start=i+2; i+=2; continue;
        }
        i+=1;
    }
    blocks.push((start, text.len()));
    let mut res=Vec::new();
    let mut cur_start=blocks[0].0; let mut cur_end=blocks[0].1;
    for b in blocks.into_iter().skip(1) {
        if line_span(text, cur_start, b.1) < target { cur_end=b.1; } else { res.push((cur_start, cur_end, preview(&text[cur_start..cur_end]))); cur_start=b.0; cur_end=b.1; }
    }
    res.push((cur_start, cur_end, preview(&text[cur_start..cur_end])));
    if overlap>0 && res.len()>1 {
        for i in 1..res.len() {
            let (s,e,pr) = res[i].clone();
            let new_s = back_n_lines(text, s, overlap);
            res[i] = (new_s, e, pr);
        }
    }
    res
}

fn back_n_lines(text: &str, mut off: usize, n: usize) -> usize { let mut k=0; while off>0 && k<n { off-=1; if text.as_bytes()[off]==b'\n' { k+=1; } } off }

fn line_span(text: &str, s: usize, e: usize) -> usize { text[s..e].bytes().filter(|b| *b==b'\n').count()+1 }

fn offset_to_line(text: &str, off: usize) -> usize { text[..off].bytes().filter(|b| *b==b'\n').count()+1 }

fn preview(chunk: &str) -> String { let mut p = chunk.lines().take(8).collect::<Vec<_>>().join("\n"); if p.len()>800 { p.truncate(800); } p }

fn chunk_rust_treesitter(text: &str, target: usize, overlap: usize) -> Option<Vec<(usize,usize,String)>> {
    let mut parser = tree_sitter::Parser::new();
    
    let lang_ts: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&lang_ts).ok()?;
    let tree = parser.parse(text, None)?;
    let root = tree.root_node();
    let mut ranges: Vec<(usize,usize)> = Vec::new();
    fn collect(node: tree_sitter::Node, out: &mut Vec<(usize,usize)>) {
        let kind = node.kind();
        if matches!(kind, "function_item"|"impl_item"|"trait_item"|"struct_item"|"enum_item"|"mod_item") {
            let r = node.range(); out.push((r.start_byte, r.end_byte));
        }
        for i in 0..node.child_count() { if let Some(ch) = node.child(i) { collect(ch, out); } }
    }
    collect(root, &mut ranges);
    if ranges.is_empty() { return None; }
    ranges.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize,usize)> = Vec::new();
    let mut cur = ranges[0];
    for r in ranges.into_iter().skip(1) {
        if line_span(text, cur.0, r.1) < target { cur.1 = r.1; } else { merged.push(cur); cur = r; }
    }
    merged.push(cur);
    let mut res = Vec::new();
    for (s,e) in merged { res.push((s,e, preview(&text[s..e]))); }
    if overlap>0 && res.len()>1 { for i in 1..res.len() { let (s,e,pr)=res[i].clone(); let new_s = back_n_lines(text, s, overlap); res[i]=(new_s,e,pr);} }
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
    let out = embedder.embed(vec![text.to_string()], None).context("embed")?;
    Ok(out.into_iter().next().unwrap_or_default())
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let mut dot=0f32; let mut na=0f32; let mut nb=0f32;
    for i in 0..a.len() { let x=a[i]; let y=b[i]; dot+=x*y; na+=x*x; nb+=y*y; }
    if na==0.0 || nb==0.0 { return 0.0; }
    dot / (na.sqrt()*nb.sqrt())
}

// ---------- analytics ----------
fn read_analytics() -> Result<Analytics> {
    let p = idx_dir().join(ANALYTICS_FILE);
    if !p.exists() { return Ok(Analytics::default()); }
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


// ---------- git & locking & time helpers ----------
fn git_has_changes() -> bool {
    let out = std::process::Command::new("git")
        .args(["ls-files","-m","-o","--exclude-standard"])
        .output();
    if let Ok(o) = out { !o.stdout.is_empty() } else { false }
}

struct BuildLock(std::path::PathBuf);
impl BuildLock {
    fn acquire() -> Result<Self> {
        let p = idx_dir().join(LOCK_FILE);
        match std::fs::File::options().create_new(true).write(true).open(&p) {
            Ok(_) => Ok(Self(p)),
            Err(e) => Err(anyhow::anyhow!(e)),
        }
    }
}
impl Drop for BuildLock { fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); } }

fn relative_age(iso: &str) -> Option<String> {
    let t = time::OffsetDateTime::parse(iso, &time::format_description::well_known::Rfc3339).ok()?;
    let now = time::OffsetDateTime::now_utc();
    let diff = now - t;
    let secs = diff.whole_seconds();
    Some(if secs < 60 { "now".into() }
         else if secs < 3600 { format!("{}m ago", secs/60) }
         else if secs < 86400 { format!("{}h ago", secs/3600) }
         else if secs < 86400*7 { format!("{}d ago", secs/86400) }
         else { format!("{}w ago", secs/(86400*7)) })
}
