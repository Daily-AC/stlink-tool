//! Auto-reflash file watcher.
//!
//! Watches the parent directory of the last-flashed file (most editors write
//! atomically by replacing the file, so watching the file itself misses the
//! event on rename). When our specific filename gets a Modify/Create/Rename
//! event, sends a single tick on `tx`. Caller debounces by ignoring ticks
//! that arrive while a flash is in progress.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::FlashError;

pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    _target: PathBuf,
}

pub fn watch(file: &Path, tx: mpsc::Sender<()>) -> Result<FileWatcher, FlashError> {
    let target = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let target_for_cb = target.clone();

    let parent = target
        .parent()
        .ok_or_else(|| FlashError::BundleError("file has no parent dir".into()))?
        .to_path_buf();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        let Ok(event) = res else { return };
        if !is_relevant(&event.kind) {
            return;
        }
        for p in &event.paths {
            // Compare canonical-ish: editors sometimes emit the new path post-rename.
            if same_file(p, &target_for_cb) {
                let _ = tx.send(());
                return;
            }
        }
    })
    .map_err(|e| FlashError::BundleError(format!("notify: {e}")))?;

    watcher
        .watch(&parent, RecursiveMode::NonRecursive)
        .map_err(|e| FlashError::BundleError(format!("watch: {e}")))?;

    Ok(FileWatcher {
        _watcher: watcher,
        _target: target,
    })
}

fn is_relevant(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Any
    )
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a.file_name() == b.file_name(),
    }
}
