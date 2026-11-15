use super::{catalog_editor::EditorSettings, theme};
use crate::{
    domain::valueobject::catalog::AnalysisCandidate,
    infra::{
        local_ai::local_stt::{self, SttConfig},
        splitter,
    },
};
use eframe::egui::{self, Context};
use std::{
    path::PathBuf,
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver},
    },
};

#[derive(Default)]
pub(super) struct Splitter {
    pub open: bool,
    input: Option<PathBuf>,
    segments: Vec<AnalysisCandidate>,
    selected: Option<usize>,
    start: f64,
    end: f64,
    task: Option<Receiver<Result<Vec<AnalysisCandidate>, String>>>,
    operation: Option<Receiver<Result<(), String>>>,
    error: Option<String>,
}

impl Splitter {
    pub fn error(&self) -> Option<&str> {
        self.open.then_some(self.error.as_deref()).flatten()
    }
    pub fn show(&mut self, ctx: &Context, settings: &EditorSettings) {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(Ok(segments)) => {
                    self.segments = segments;
                    self.selected = None;
                    self.task = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.task = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("解析が中断されました。".into());
                    self.task = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200))
                }
            }
        }
        if let Some(task) = &self.operation {
            match task.try_recv() {
                Ok(result) => {
                    self.error = result.err();
                    self.operation = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("処理が中断されました。".into());
                    self.operation = None;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200))
                }
            }
        }
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("動画から素材を切り出す")
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                let busy = self.task.is_some() || self.operation.is_some();
                ui.add_enabled_ui(!busy, |ui| {
                    if theme::button(ui, "動画を選ぶ", theme::ButtonKind::Normal).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_file()
                    {
                        self.input = Some(path);
                        self.segments.clear();
                        self.selected = None;
                        self.error = None;
                    }
                    if let Some(path) = &self.input {
                        ui.label(path.display().to_string());
                    }
                    if ui
                        .add_enabled(self.input.is_some(), egui::Button::new("台詞を解析"))
                        .clicked()
                    {
                        let input = self.input.clone().unwrap_or_default();
                        let config = SttConfig {
                            ffmpeg: "ffmpeg".into(),
                            whisper: settings.whisper_path.clone().into(),
                            model: settings.model_path.clone().into(),
                            language: settings.speech_language,
                        };
                        let (tx, rx) = mpsc::channel();
                        self.task = Some(rx);
                        self.segments.clear();
                        self.selected = None;
                        self.error = None;
                        std::thread::spawn(move || {
                            let cancel = AtomicBool::new(false);
                            let result = local_stt::fingerprint(&input, &cancel)
                                .and_then(|hash| {
                                    local_stt::transcribe(&config, &input, &hash, &cancel)
                                })
                                .map_err(|e| e.to_string());
                            let _ = tx.send(result);
                        });
                    }
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for (index, segment) in self.segments.iter().enumerate() {
                                if ui
                                    .selectable_label(
                                        self.selected == Some(index),
                                        format!(
                                            "{:.2}–{:.2} 秒  {}",
                                            segment.start_ms as f64 / 1000.0,
                                            segment.end_ms as f64 / 1000.0,
                                            segment.text
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.selected = Some(index);
                                    self.start = segment.start_ms as f64 / 1000.0;
                                    self.end = segment.end_ms as f64 / 1000.0;
                                }
                            }
                        });
                    if self.selected.is_some() {
                        ui.horizontal(|ui| {
                            ui.label("開始（秒）");
                            ui.add(
                                egui::DragValue::new(&mut self.start)
                                    .speed(0.1)
                                    .range(0.0..=f64::MAX),
                            );
                            ui.label("終了（秒）");
                            ui.add(
                                egui::DragValue::new(&mut self.end)
                                    .speed(0.1)
                                    .range(0.0..=f64::MAX),
                            );
                        });
                        ui.horizontal(|ui| {
                            if theme::button(ui, "再生", theme::ButtonKind::Normal).clicked() {
                                self.run_operation(None);
                            }
                            if theme::button(ui, "WAVに保存", theme::ButtonKind::Primary).clicked()
                                && let Some(path) = rfd::FileDialog::new()
                                    .add_filter("WAV", &["wav"])
                                    .set_file_name("material.wav")
                                    .save_file()
                            {
                                self.run_operation(Some(path));
                            }
                        });
                    }
                });
                if busy {
                    ui.label("処理中…");
                }
                if let Some(error) = &self.error {
                    theme::state_message(ui, theme::State::Error, error);
                }
            });
        self.open = open;
    }
    fn run_operation(&mut self, output: Option<PathBuf>) {
        let Some(input) = self.input.clone() else {
            return;
        };
        let (start, end) = (self.start, self.end);
        let (tx, rx) = mpsc::channel();
        self.operation = Some(rx);
        self.error = None;
        std::thread::spawn(move || {
            let result = if let Some(output) = output {
                splitter::cut(&input, &output, start, end)
            } else {
                tempfile::tempdir()
                    .map_err(|e| e.to_string())
                    .and_then(|dir| {
                        let wav = dir.path().join("preview.wav");
                        splitter::cut(&input, &wav, start, end)?;
                        play(&wav)
                    })
            };
            let _ = tx.send(result);
        });
    }
}

#[cfg(windows)]
fn play(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let status = std::process::Command::new("powershell.exe")
        .creation_flags(0x08000000)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$ErrorActionPreference = 'Stop'; (New-Object System.Media.SoundPlayer $env:CUTUP_PREVIEW_FILE).PlaySync()",
        ])
        .env("CUTUP_PREVIEW_FILE", path)
        .status()
        .map_err(|e| format!("再生を開始できません: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("音声を再生できません。".into())
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "requires a working Windows audio device"]
    fn preview_plays_a_wave_file() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/materials/01-tekkaman.wav");
        super::play(&path).unwrap();
    }
}
#[cfg(not(windows))]
fn play(path: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("ffplay")
        .args(["-nodisp", "-autoexit", "-v", "error"])
        .arg(path)
        .status()
        .map_err(|e| format!("ffplay を起動できません: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("音声を再生できません。".into())
    }
}
