//! SHA-256 integrity: manifest + verification on the retrieval side.
//! Streaming hash (1 MiB chunks) -> constant memory even for multi-GB files.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Label (file name / relative path) -> lowercase hex SHA-256.
pub type Manifest = BTreeMap<String, String>;

/// Streaming SHA-256 of a file.
pub fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn collect_dir(dir: &Path, base: &Path, out: &mut Vec<(String, PathBuf)>) -> anyhow::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_dir(&p, base, out)?;
        } else {
            let label = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().replace('\\', "/");
            out.push((label, p));
        }
    }
    Ok(())
}

/// Builds a manifest from local files and/or directories.
/// Label: base name for a file, path relative to the directory for a directory.
pub fn build_manifest(paths: &[PathBuf]) -> anyhow::Result<Manifest> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for p in paths {
        if p.is_dir() {
            collect_dir(p, p, &mut files)?;
        } else {
            let label = p
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string());
            files.push((label, p.clone()));
        }
    }
    let mut m = Manifest::new();
    for (label, path) in files {
        m.insert(label, sha256_file(&path)?);
    }
    Ok(m)
}

pub fn manifest_to_pretty_json(m: &Manifest) -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(m)?)
}

pub fn load_manifest(path: &Path) -> anyhow::Result<Manifest> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[derive(Debug, Default)]
pub struct VerifyReport {
    /// (object key, matching manifest label(s))
    pub matched: Vec<(String, String)>,
    /// received objects whose hash isn't in the manifest (key, sha256)
    pub unexpected: Vec<(String, String)>,
    /// manifest entries whose hash wasn't received (label, sha256)
    pub missing: Vec<(String, String)>,
}

impl VerifyReport {
    pub fn is_ok(&self) -> bool {
        self.unexpected.is_empty() && self.missing.is_empty()
    }
}

/// Verifies downloaded objects (key, local path) against a manifest, **by content**
/// (the hash), regardless of the key name (which carries a `data/` prefix and a
/// timestamp not known in advance).
pub fn verify(
    downloaded: &[(String, PathBuf)],
    manifest: &Manifest,
) -> anyhow::Result<VerifyReport> {
    let mut by_hash: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (label, h) in manifest {
        by_hash.entry(h.to_lowercase()).or_default().push(label.clone());
    }
    let mut report = VerifyReport::default();
    let mut seen: HashSet<String> = HashSet::new();
    for (key, path) in downloaded {
        let h = sha256_file(path)?.to_lowercase();
        seen.insert(h.clone());
        match by_hash.get(&h) {
            Some(labels) => report.matched.push((key.clone(), labels.join(", "))),
            None => report.unexpected.push((key.clone(), h)),
        }
    }
    for (label, h) in manifest {
        if !seen.contains(&h.to_lowercase()) {
            report.missing.push((label.clone(), h.clone()));
        }
    }
    Ok(report)
}
