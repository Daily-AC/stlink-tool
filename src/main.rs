// stlink-tool: drag-and-drop STM32F10x flasher with self-healing ST-Link
// driver on Windows. See README.md and docs/superpowers/specs/ for design.

#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bundle;
mod device;
mod driver_fix;
mod error;
mod flasher;
mod watcher;

fn main() -> eframe::Result<()> {
    init_tracing();

    let bundle = match bundle::ensure() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to extract bundled resources: {e}");
            std::process::exit(1);
        }
    };

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([520.0, 480.0])
            .with_min_inner_size([420.0, 380.0])
            .with_drag_and_drop(true)
            .with_title("stlink-tool"),
        ..Default::default()
    };

    eframe::run_native(
        "stlink-tool",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, bundle)))),
    )
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("STLINK_TOOL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
