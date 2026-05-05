// build.rs — embed Windows app manifest (asInvoker, DPI aware, longPathAware)
// On non-Windows targets this is a no-op so the project still cargo-checks on macOS/Linux.

#[cfg(target_os = "windows")]
fn main() {
    use embed_manifest::{embed_manifest, new_manifest};

    let manifest = new_manifest("StlinkTool")
        // Default execution level — only the driver-fix step elevates via ShellExecute("runas").
        // Keeping the main process unprivileged means file watching, Logs, and openocd flashing
        // all run under the user's normal token (WinUSB devices are user-accessible).
        .ui_access(false);

    if let Err(e) = embed_manifest(manifest) {
        eprintln!("cargo:warning=Failed to embed manifest: {e}");
    }

    println!("cargo:rerun-if-changed=build.rs");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
}
