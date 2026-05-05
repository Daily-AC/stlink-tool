//! egui application — state machine + rendering.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::bundle::Bundle;
use crate::device::{self, DetectResult};
use crate::driver_fix;
use crate::error::FlashError;
use crate::flasher;

const LOG_RING_CAPACITY: usize = 400;

#[derive(Debug)]
enum AppState {
    Idle,
    Working(WorkStage),
    Done(Duration),
    Error(String),
}

#[derive(Debug, Clone, Copy)]
enum WorkStage {
    Validating,
    Detecting,
    FixingDriver,
    Flashing,
}

impl WorkStage {
    fn label(self) -> &'static str {
        match self {
            Self::Validating => "Validating file",
            Self::Detecting => "Detecting ST-Link",
            Self::FixingDriver => "Installing WinUSB driver (UAC will prompt)",
            Self::Flashing => "Flashing",
        }
    }
}

#[derive(Debug)]
enum Msg {
    Log(String),
    Stage(WorkStage),
    Done(Duration),
    Error(String),
}

pub struct App {
    state: AppState,
    last_file: Option<PathBuf>,
    auto_reflash: bool,
    log: VecDeque<String>,
    bundle: Arc<Bundle>,
    rt: Runtime,
    msg_tx: mpsc::UnboundedSender<Msg>,
    msg_rx: mpsc::UnboundedReceiver<Msg>,
    watch_tx: std::sync::mpsc::Sender<()>,
    watch_rx: std::sync::mpsc::Receiver<()>,
    _watcher: Option<crate::watcher::FileWatcher>,
    egui_ctx: egui::Context,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, bundle: Bundle) -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let (watch_tx, watch_rx) = std::sync::mpsc::channel();
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime");

        Self {
            state: AppState::Idle,
            last_file: None,
            auto_reflash: true,
            log: VecDeque::with_capacity(LOG_RING_CAPACITY),
            bundle: Arc::new(bundle),
            rt,
            msg_tx,
            msg_rx,
            watch_tx,
            watch_rx,
            _watcher: None,
            egui_ctx: cc.egui_ctx.clone(),
        }
    }

    fn append_log(&mut self, line: String) {
        if self.log.len() == LOG_RING_CAPACITY {
            self.log.pop_front();
        }
        self.log.push_back(line);
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.msg_rx.try_recv() {
            match msg {
                Msg::Log(l) => self.append_log(l),
                Msg::Stage(s) => {
                    self.append_log(format!("[*] {}", s.label()));
                    self.state = AppState::Working(s);
                }
                Msg::Done(elapsed) => {
                    self.append_log(format!("[✓] Flashed in {:.1}s", elapsed.as_secs_f32()));
                    self.state = AppState::Done(elapsed);
                }
                Msg::Error(e) => {
                    self.append_log(format!("[x] {e}"));
                    self.state = AppState::Error(e);
                }
            }
        }

        // Watcher ticks → auto re-flash if eligible.
        let mut tick = false;
        while self.watch_rx.try_recv().is_ok() {
            tick = true;
        }
        if tick && self.auto_reflash && self.eligible_for_auto_reflash() {
            if let Some(file) = self.last_file.clone() {
                self.append_log(format!(
                    "[~] {} changed — auto re-flashing",
                    short_name(&file)
                ));
                self.spawn_pipeline(file);
            }
        }
    }

    fn eligible_for_auto_reflash(&self) -> bool {
        matches!(
            self.state,
            AppState::Idle | AppState::Done(_) | AppState::Error(_)
        )
    }

    fn handle_dropped(&mut self, file: PathBuf) {
        self.append_log(format!("[~] dropped: {}", short_name(&file)));
        self.last_file = Some(file.clone());
        self.install_watcher(&file);
        self.spawn_pipeline(file);
    }

    fn install_watcher(&mut self, file: &std::path::Path) {
        match crate::watcher::watch(file, self.watch_tx.clone()) {
            Ok(w) => self._watcher = Some(w),
            Err(e) => {
                self.append_log(format!("[!] file watcher disabled: {e}"));
                self._watcher = None;
            }
        }
    }

    fn spawn_pipeline(&mut self, file: PathBuf) {
        let bundle = self.bundle.clone();
        let tx = self.msg_tx.clone();
        let ctx = self.egui_ctx.clone();

        self.rt.spawn(async move {
            let result = run_pipeline(bundle, file, tx.clone()).await;
            if let Err(e) = result {
                let _ = tx.send(Msg::Error(e.to_string()));
            }
            ctx.request_repaint();
        });
    }
}

async fn run_pipeline(
    bundle: Arc<Bundle>,
    file: PathBuf,
    tx: mpsc::UnboundedSender<Msg>,
) -> Result<(), FlashError> {
    let _ = tx.send(Msg::Stage(WorkStage::Validating));
    let kind = flasher::validate(&file)?;

    let _ = tx.send(Msg::Stage(WorkStage::Detecting));
    let detect = detect_blocking().await?;

    let info = match detect {
        DetectResult::None => return Err(FlashError::NoStlinkDevice),
        DetectResult::Ready(info) => info,
        DetectResult::NeedsDriverFix(info) => {
            let _ = tx.send(Msg::Log(format!(
                "[!] {} found but driver not bound to WinUSB",
                info.variant.label()
            )));
            let _ = tx.send(Msg::Stage(WorkStage::FixingDriver));

            let bundle_for_fix = bundle.clone();
            let vid = info.vid;
            let pid = info.pid;
            tokio::task::spawn_blocking(move || {
                driver_fix::install_winusb_blocking(&bundle_for_fix, vid, pid)
            })
            .await
            .map_err(|e| FlashError::BundleError(format!("driver fix task: {e}")))??;

            let _ = tx.send(Msg::Log("[+] WinUSB installed; verifying".into()));
            match detect_blocking().await? {
                DetectResult::Ready(info) => info,
                _ => return Err(FlashError::DriverFixIneffective),
            }
        }
    };

    let _ = tx.send(Msg::Log(format!(
        "[+] {} ready (sn={})",
        info.variant.label(),
        info.serial.as_deref().unwrap_or("?")
    )));

    let _ = tx.send(Msg::Stage(WorkStage::Flashing));

    // Bridge openocd's String log channel into our Msg channel.
    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<String>();
    let tx_log = tx.clone();
    tokio::spawn(async move {
        while let Some(line) = log_rx.recv().await {
            let _ = tx_log.send(Msg::Log(line));
        }
    });

    let elapsed = flasher::flash(&bundle, &file, kind, log_tx).await?;
    let _ = tx.send(Msg::Done(elapsed));
    Ok(())
}

async fn detect_blocking() -> Result<DetectResult, FlashError> {
    tokio::task::spawn_blocking(device::detect)
        .await
        .map_err(|e| FlashError::UsbError(format!("detect task: {e}")))?
}

fn short_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| p.display().to_string())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_messages();

        let ctx = ui.ctx().clone();
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for f in dropped {
            if let Some(p) = f.path {
                self.handle_dropped(p);
            }
        }

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            self.render_drop_zone(ui);
            ui.add_space(8.0);
            self.render_status_row(ui);
            ui.add_space(8.0);
            self.render_log(ui);
        });

        ctx.request_repaint_after(Duration::from_millis(120));
    }
}

impl App {
    fn render_drop_zone(&mut self, ui: &mut egui::Ui) {
        let (heading, sub) = match &self.state {
            AppState::Idle => (
                "⬇  Drop .hex / .bin / .elf".to_string(),
                "to flash STM32F10x".to_string(),
            ),
            AppState::Working(stage) => {
                (format!("⏳  {}", stage.label()), "please wait…".to_string())
            }
            AppState::Done(d) => (
                format!("✓  Flashed in {:.1}s", d.as_secs_f32()),
                "drop another file or rebuild to re-flash".to_string(),
            ),
            AppState::Error(msg) => (format!("✗  {msg}"), "drop a file to retry".to_string()),
        };

        let frame = egui::Frame::group(ui.style())
            .fill(zone_fill(&self.state, ui.visuals()))
            .stroke(egui::Stroke::new(
                1.5,
                ui.visuals().widgets.noninteractive.fg_stroke.color,
            ));

        frame.show(ui, |ui| {
            ui.set_min_height(140.0);
            ui.vertical_centered(|ui| {
                ui.add_space(36.0);
                ui.label(egui::RichText::new(heading).size(20.0).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new(sub).size(13.0).weak());
                ui.add_space(36.0);
            });
        });
    }

    fn render_status_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let last = self
                .last_file
                .as_deref()
                .map(short_name)
                .unwrap_or_else(|| "—".into());
            ui.label(format!("Last: {last}"));
            ui.add_space(16.0);
            ui.checkbox(&mut self.auto_reflash, "Auto-reflash on file change");
        });
    }

    fn render_log(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Log").weak().small());
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &self.log {
                    ui.add(egui::Label::new(egui::RichText::new(line).monospace()).wrap());
                }
            });
    }
}

fn zone_fill(state: &AppState, vis: &egui::Visuals) -> egui::Color32 {
    match state {
        AppState::Done(_) => egui::Color32::from_rgb(36, 90, 50),
        AppState::Error(_) => egui::Color32::from_rgb(110, 40, 40),
        AppState::Working(_) => vis.faint_bg_color,
        AppState::Idle => vis.extreme_bg_color,
    }
}
