//! OpenOCD subprocess wrapper.
//!
//! Runs the bundled `openocd.exe` with the F10x target script and streams
//! every output line back through `log_tx`. The flash address is taken from
//! the file (.hex / .elf carry their own) or hardcoded to 0x08000000 for raw
//! .bin (the F10x flash base for every chip in the family).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::bundle::Bundle;
use crate::error::FlashError;

const F10X_FLASH_BASE: &str = "0x08000000";
const F10X_MAX_FLASH_BYTES: u64 = 128 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum FileKind {
    Hex,
    Bin,
    Elf,
}

impl FileKind {
    pub fn from_path(p: &Path) -> Option<Self> {
        let ext = p.extension()?.to_str()?.to_ascii_lowercase();
        match ext.as_str() {
            "hex" => Some(Self::Hex),
            "bin" => Some(Self::Bin),
            "elf" => Some(Self::Elf),
            _ => None,
        }
    }
}

/// Pre-flash sanity check. Cheap, runs on the GUI thread before kicking work.
pub fn validate(file: &Path) -> Result<FileKind, FlashError> {
    let kind = FileKind::from_path(file).ok_or(FlashError::BadExtension)?;
    let meta = std::fs::metadata(file).map_err(|_| FlashError::FileMissing {
        path: file.display().to_string(),
    })?;
    if meta.len() == 0 {
        return Err(FlashError::FileEmpty);
    }
    // Only enforce the F10x flash ceiling on raw .bin where size == flash usage.
    // For .hex and .elf, file size includes overhead — let openocd verify.
    if matches!(kind, FileKind::Bin) && meta.len() > F10X_MAX_FLASH_BYTES {
        return Err(FlashError::FileTooLarge { size: meta.len() });
    }
    Ok(kind)
}

pub fn build_args(bundle: &Bundle, file: &Path, kind: FileKind) -> (PathBuf, Vec<String>) {
    let scripts = bundle.openocd_scripts.display().to_string();
    let file_arg = openocd_quote(file);

    let prog_cmd = match kind {
        FileKind::Bin => format!("program {file_arg} {F10X_FLASH_BASE} verify reset exit"),
        FileKind::Hex | FileKind::Elf => format!("program {file_arg} verify reset exit"),
    };

    let args = vec![
        "-s".into(),
        scripts,
        "-f".into(),
        "interface/stlink.cfg".into(),
        "-f".into(),
        "target/stm32f1x.cfg".into(),
        "-c".into(),
        prog_cmd,
    ];
    (bundle.openocd_exe.clone(), args)
}

/// Wrap a path in {curly braces} — OpenOCD's TCL parser treats braces as a
/// quote-stop, so spaces and Windows backslashes survive verbatim.
fn openocd_quote(path: &Path) -> String {
    let s = path.display().to_string();
    format!("{{{s}}}")
}

pub async fn flash(
    bundle: &Bundle,
    file: &Path,
    kind: FileKind,
    log_tx: mpsc::UnboundedSender<String>,
) -> Result<Duration, FlashError> {
    let start = Instant::now();
    let (exe, args) = build_args(bundle, file, kind);

    let mut child = Command::new(&exe)
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| FlashError::OpenocdFailed(format!("spawn: {e}")))?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let collected = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

    let stdout_handle = spawn_reader("stdout", stdout, log_tx.clone(), collected.clone());
    let stderr_handle = spawn_reader("stderr", stderr, log_tx.clone(), collected.clone());

    let status = child
        .wait()
        .await
        .map_err(|e| FlashError::OpenocdFailed(format!("wait: {e}")))?;
    let _ = stdout_handle.await;
    let _ = stderr_handle.await;

    if status.success() {
        Ok(start.elapsed())
    } else {
        let log = collected.lock().unwrap().clone();
        Err(FlashError::from_openocd_log(&log))
    }
}

fn spawn_reader<R>(
    label: &'static str,
    reader: R,
    tx: mpsc::UnboundedSender<String>,
    sink: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buf = BufReader::new(reader).lines();
        while let Ok(Some(line)) = buf.next_line().await {
            sink.lock().unwrap().push(line.clone());
            let _ = tx.send(line);
        }
        tracing::trace!("{label} closed");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn detects_extensions_case_insensitive() {
        assert!(matches!(FileKind::from_path(Path::new("a.hex")), Some(FileKind::Hex)));
        assert!(matches!(FileKind::from_path(Path::new("a.HEX")), Some(FileKind::Hex)));
        assert!(matches!(FileKind::from_path(Path::new("a.bin")), Some(FileKind::Bin)));
        assert!(matches!(FileKind::from_path(Path::new("a.elf")), Some(FileKind::Elf)));
        assert!(FileKind::from_path(Path::new("a.txt")).is_none());
        assert!(FileKind::from_path(Path::new("noext")).is_none());
    }

    #[test]
    fn validate_rejects_unknown_extension() {
        let p = std::env::temp_dir().join("stlink-tool-test-bad.txt");
        std::fs::write(&p, b"hi").unwrap();
        let r = validate(&p);
        let _ = std::fs::remove_file(&p);
        assert!(matches!(r, Err(FlashError::BadExtension)));
    }

    #[test]
    fn validate_rejects_empty_bin() {
        let p = std::env::temp_dir().join("stlink-tool-test-empty.bin");
        std::fs::write(&p, b"").unwrap();
        let r = validate(&p);
        let _ = std::fs::remove_file(&p);
        assert!(matches!(r, Err(FlashError::FileEmpty)));
    }

    #[test]
    fn validate_rejects_oversize_bin() {
        let p = std::env::temp_dir().join("stlink-tool-test-big.bin");
        let big = vec![0u8; (F10X_MAX_FLASH_BYTES + 1) as usize];
        std::fs::write(&p, &big).unwrap();
        let r = validate(&p);
        let _ = std::fs::remove_file(&p);
        assert!(matches!(r, Err(FlashError::FileTooLarge { .. })));
    }

    #[test]
    fn quote_wraps_path_in_braces() {
        let q = openocd_quote(Path::new("C:/Users/dev/blink with spaces.hex"));
        assert!(q.starts_with('{') && q.ends_with('}'));
        assert!(q.contains("blink with spaces.hex"));
    }
}
