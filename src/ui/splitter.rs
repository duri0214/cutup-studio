use super::{
    catalog_editor::{EditorSettings, executable_exists},
    theme,
};
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

type AnalysisTask =
    Receiver<Result<(Vec<AnalysisCandidate>, Option<tempfile::TempDir>, PathBuf), String>>;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum InputSource {
    #[default]
    Youtube,
    Local,
}

#[derive(Default)]
pub(super) struct Splitter {
    pub open: bool,
    source: InputSource,
    input: Option<PathBuf>,
    url: String,
    downloaded: Option<tempfile::TempDir>,
    segments: Vec<AnalysisCandidate>,
    selected: Option<usize>,
    start: f64,
    end: f64,
    task: Option<AnalysisTask>,
    operation: Option<Receiver<Result<(), String>>>,
    error: Option<String>,
}

impl Splitter {
    fn set_source(&mut self, source: InputSource) {
        if self.source != source {
            self.source = source;
            self.input = None;
            self.url.clear();
            self.downloaded = None;
            self.segments.clear();
            self.selected = None;
            self.error = None;
        }
    }
    pub fn error(&self) -> Option<&str> {
        self.open.then_some(self.error.as_deref()).flatten()
    }
    pub fn poll(&mut self, ctx: &Context) {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(Ok((segments, downloaded, input))) => {
                    self.segments = segments;
                    self.input = Some(input);
                    self.downloaded = downloaded;
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
    }
    pub fn show(&mut self, ui: &mut egui::Ui, settings: &EditorSettings) {
        theme::panel().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(theme::section_title("動画から素材を切り出す"));
                    if theme::button(ui, "閉じる", theme::ButtonKind::Normal).clicked() {
                        self.open = false;
                    }
                });
                let busy = self.task.is_some() || self.operation.is_some();
                ui.label("必要なツール（ファイルの有無）");
                egui::Grid::new("splitter_tool_status").show(ui, |ui| {
                    for (name, present) in [
                        ("FFmpeg", executable_exists("ffmpeg")),
                        ("Whisper", executable_exists(&settings.whisper_path)),
                        ("モデル", std::path::Path::new(&settings.model_path).is_file()),
                        ("yt-dlp（URL用）", executable_exists("yt-dlp")),
                    ] {
                        theme::availability_indicator(ui, present);
                        ui.label(name);
                        ui.label(if present { "検出" } else { "未検出" });
                        ui.end_row();
                    }
                });
                ui.label("Whisper とモデルは素材の「設定」で指定できます。PATH 変更後はアプリを再起動してください。");
                ui.add_enabled_ui(!busy, |ui| {
                    ui.label("動画の入力元");
                    ui.horizontal(|ui| {
                        if ui.radio(self.source == InputSource::Youtube, "YouTube URL").clicked() {
                            self.set_source(InputSource::Youtube);
                        }
                        if ui.radio(self.source == InputSource::Local, "ローカル動画").clicked() {
                            self.set_source(InputSource::Local);
                        }
                    });
                    match self.source {
                        InputSource::Youtube => {
                            ui.label("URL の音声を一時取得して解析します。yt-dlp が必要です。");
                            let width = ui.available_width().max(120.0);
                            if theme::text_input(ui, &mut self.url, "YouTube の動画 URL", width).changed() {
                                self.input = None;
                                self.downloaded = None;
                                self.segments.clear();
                                self.selected = None;
                                self.error = None;
                            }
                        }
                        InputSource::Local => {
                            if theme::button(ui, "ファイルを選ぶ", theme::ButtonKind::Normal).clicked()
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
                        }
                    }
                    if ui
                        .add_enabled(
                            self.input.is_some() || !self.url.trim().is_empty(),
                            egui::Button::new("音声を抽出・文字起こし"),
                        )
                        .clicked()
                    {
                        let local_input = self.input.clone();
                        let url = self.url.trim().to_owned();
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
                            let result = (|| {
                                let downloaded = if url.is_empty() {
                                    None
                                } else {
                                    Some(splitter::download_youtube(&url)?)
                                };
                                let input = if let Some(dir) = &downloaded {
                                    dir.path().join("input.wav")
                                } else {
                                    local_input.ok_or("ローカル動画を選んでください。")?
                                };
                                let hash = local_stt::fingerprint(&input, &cancel)
                                    .map_err(|e| e.to_string())?;
                                let segments =
                                    local_stt::transcribe(&config, &input, &hash, &cancel)
                                        .map_err(|e| e.to_string())?;
                                Ok((segments, downloaded, input))
                            })();
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

#[cfg(test)]
mod tests {
    #[test]
    fn changing_source_discards_the_previous_selection() {
        let mut splitter = super::Splitter {
            url: "https://youtu.be/jNQXAC9IVRw".into(),
            input: Some("input.wav".into()),
            selected: Some(0),
            ..Default::default()
        };
        splitter.set_source(super::InputSource::Local);
        assert!(splitter.url.is_empty());
        assert!(splitter.input.is_none());
        assert!(splitter.selected.is_none());
        splitter.input = Some("local.wav".into());
        splitter.set_source(super::InputSource::Local);
        assert_eq!(
            splitter.input.as_deref(),
            Some(std::path::Path::new("local.wav"))
        );
        splitter.set_source(super::InputSource::Youtube);
        assert!(splitter.input.is_none());
    }

    #[cfg(windows)]
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
