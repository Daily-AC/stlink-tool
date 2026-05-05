//! Embedded vendor blobs (wdi-simple + xpack-openocd) extracted to a
//! per-version cache directory under `%LOCALAPPDATA%\stlink-tool\bundle-<sha8>\`.
//!
//! Cache key = SHA-256 of the concatenated embedded bytes, truncated to 8 hex
//! chars. Different tool versions produce different keys, so upgrades don't
//! collide; the GC step removes any sibling `bundle-*` directory that isn't
//! the active one.

use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::FlashError;

const WDI_SIMPLE_EXE: &[u8] = include_bytes!("../vendor/wdi-simple.exe");
const OPENOCD_ZIP: &[u8] = include_bytes!("../vendor/xpack-openocd-0.12.0-7-win32-x64.zip");

#[derive(Clone, Debug)]
pub struct Bundle {
    pub wdi_simple_exe: PathBuf,
    pub openocd_exe: PathBuf,
    pub openocd_scripts: PathBuf,
}

pub fn ensure() -> Result<Bundle, FlashError> {
    let key = bundle_key();
    let root = cache_root()?;
    let dir = root.join(format!("bundle-{key}"));
    let marker = dir.join(".ready");

    if !marker.exists() {
        if dir.exists() {
            // Half-extracted from a previous crashed run — start fresh.
            let _ = fs::remove_dir_all(&dir);
        }
        fs::create_dir_all(&dir)?;
        extract_into(&dir)?;
        fs::write(&marker, key.as_bytes())?;
        gc_old_bundles(&root, &dir);
    }

    let openocd_root = dir.join("xpack-openocd-0.12.0-7");
    Ok(Bundle {
        wdi_simple_exe: dir.join("wdi-simple.exe"),
        openocd_exe: openocd_root.join("bin").join("openocd.exe"),
        openocd_scripts: openocd_root.join("scripts"),
    })
}

fn bundle_key() -> String {
    let mut h = Sha256::new();
    h.update(WDI_SIMPLE_EXE);
    h.update(OPENOCD_ZIP);
    let digest = h.finalize();
    hex8(&digest[..])
}

fn hex8(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(8);
    for b in bytes.iter().take(4) {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn cache_root() -> Result<PathBuf, FlashError> {
    let dirs = directories::ProjectDirs::from("", "", "stlink-tool")
        .ok_or_else(|| FlashError::BundleError("could not resolve cache dir".into()))?;
    Ok(dirs.cache_dir().to_path_buf())
}

fn extract_into(dir: &Path) -> Result<(), FlashError> {
    write_file(&dir.join("wdi-simple.exe"), WDI_SIMPLE_EXE)?;

    let cursor = Cursor::new(OPENOCD_ZIP);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|e| FlashError::BundleError(format!("openocd zip header: {e}")))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| FlashError::BundleError(format!("openocd zip entry {i}: {e}")))?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let outpath = dir.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut buf = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut buf)?;
            write_file(&outpath, &buf)?;
        }
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), FlashError> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    Ok(())
}

fn gc_old_bundles(root: &Path, keep: &Path) {
    let Ok(entries) = fs::read_dir(root) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p == keep {
            continue;
        }
        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if name.starts_with("bundle-") {
            let _ = fs::remove_dir_all(&p);
        }
    }
}
