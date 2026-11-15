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

type AnalysisTask = Receiver<
    Result<
        (
            Vec<AnalysisCandidate>,
            Option<tempfile::TempDir>,
            PathBuf,
            f64,
        ),
        String,
    >,
>;

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
    reset_result_scroll: bool,
    result_ready: bool,
    duration: f64,
    selected: Option<usize>,
    start: f64,
    end: f64,
    task: Option<AnalysisTask>,
    operation: Option<Receiver<Result<Option<PathBuf>, String>>>,
    saved: Option<PathBuf>,
    rescan_pending: bool,
    error: Option<String>,
}

impl Splitter {
    pub fn is_busy(&self) -> bool {
        self.task.is_some() || self.operation.is_some()
    }
    fn set_source(&mut self, source: InputSource) {
        if self.source != source {
            self.source = source;
            self.input = None;
            self.url.clear();
            self.downloaded = None;
            self.segments.clear();
            self.result_ready = false;
            self.selected = None;
            self.error = None;
            self.saved = None;
        }
    }
    pub fn result_count(&self) -> Option<usize> {
        (self.open && self.result_ready && !self.is_busy()).then_some(self.segments.len())
    }
    pub fn take_rescan_pending(&mut self) -> bool {
        std::mem::take(&mut self.rescan_pending)
    }
    pub fn rescan_failed(&mut self) {
        self.saved = None;
    }
    pub fn saved_path(&self) -> Option<&std::path::Path> {
        self.open.then_some(self.saved.as_deref()).flatten()
    }
    pub fn error(&self) -> Option<&str> {
        self.open.then_some(self.error.as_deref()).flatten()
    }
    pub fn poll(&mut self, ctx: &Context) {
        if let Some(task) = &self.task {
            match task.try_recv() {
                Ok(Ok((segments, downloaded, input, duration))) => {
                    self.segments = valid_segments(segments, duration);
                    self.reset_result_scroll = true;
                    self.result_ready = true;
                    self.duration = duration;
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
                    match result {
                        Ok(path) => {
                            if path.is_some() {
                                self.saved = path;
                                self.rescan_pending = true;
                            }
                        }
                        Err(error) => self.error = Some(error),
                    }
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
    pub fn show(&mut self, ui: &mut egui::Ui, settings: &EditorSettings, root: &std::path::Path) {
        theme::panel().show(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::pane_title(ui, "動画から素材を切り出す");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::icon_button(ui, theme::ActionIcon::Close, "閉じる", theme::ButtonKind::Normal).clicked() {
                            self.open = false;
                        }
                    });
                });
                let busy = self.task.is_some() || self.operation.is_some();
                ui.label("必要なツール（ファイルの有無）");
                egui::Grid::new("splitter_tool_status").show(ui, |ui| {
                    for (name, present) in [
                        ("FFmpeg", executable_exists("ffmpeg")),
                        ("Whisper", executable_exists(&settings.whisper_path)),
                        ("モデル", std::path::Path::new(&settings.model_path).is_file()),
                        ("yt-dlp（動画から音声を取得）", executable_exists("yt-dlp")),
                    ] {
                        theme::availability_indicator(ui, present).on_hover_text(if present { "利用可能" } else { "見つかりません" });
                        ui.label(name);
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
                    let mut submit_url = false;
                    match self.source {
                        InputSource::Youtube => {
                            let width = ui.available_width().max(120.0);
                            let response = theme::text_input(ui, &mut self.url, "YouTube の動画 URL", width);
                            submit_url = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) && !self.url.trim().is_empty();
                            if response.changed() {
                                self.input = None;
                                self.downloaded = None;
                                self.segments.clear();
            self.result_ready = false;
                                self.selected = None;
                                self.error = None;
                                self.saved = None;
                            }
                        }
                        InputSource::Local => {
                            if theme::button(ui, "ファイルを選ぶ", theme::ButtonKind::Normal).clicked()
                                && let Some(path) = rfd::FileDialog::new().pick_file()
                            {
                                self.input = Some(path);
                                self.segments.clear();
            self.result_ready = false;
                                self.selected = None;
                                self.error = None;
                                self.saved = None;
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
                        .clicked() || submit_url
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
            self.result_ready = false;
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
                                let duration = splitter::duration_seconds(&input)?;
                                let hash = local_stt::fingerprint(&input, &cancel)
                                    .map_err(|e| e.to_string())?;
                                let segments =
                                    local_stt::transcribe_for_splitting(&config, &input, &hash, &cancel)
                                        .map_err(|e| e.to_string())?;
                                Ok((segments, downloaded, input, duration))
                            })();
                            let _ = tx.send(result);
                        });
                    }
                    if !self.segments.is_empty() {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(8.0);
                        let reset_scroll = std::mem::take(&mut self.reset_result_scroll);
                        let heading = ui.label("文字起こし結果");
                        if reset_scroll {
                            heading.scroll_to_me(Some(egui::Align::Min));
                        }
                        if theme::button(ui, "すべて保存", theme::ButtonKind::Primary).clicked() {
                            self.save_all(root.to_owned());
                        }
                        let mut action = None;
                        results_scroll_area(reset_scroll).show(ui, |ui| {
                            let label_width = 150.0_f32.min((ui.available_width() * 0.4).max(40.0));
                            for (index, segment) in self.segments.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    if segment_label(ui, &segment.text, self.selected == Some(index), label_width).clicked() {
                                        self.selected = Some(index);
                                        self.start = segment.start_ms as f64 / 1000.0;
                                        self.end = segment.end_ms as f64 / 1000.0;
                                    }
                                    let mut start = segment.start_ms as f64 / 1000.0;
                                    let mut end = segment.end_ms as f64 / 1000.0;
                                    if range_slider(ui, index, self.duration, &mut start, &mut end) {
                                        segment.start_ms = (start * 1000.0).round() as u64;
                                        segment.end_ms = (end * 1000.0).round() as u64;
                                        self.selected = Some(index);
                                        self.start = start;
                                        self.end = end;
                                    }
                                    if theme::icon_button(ui, theme::ActionIcon::Play, "再生", theme::ButtonKind::Normal).clicked() { action = Some((index, false)); }
                                    if theme::icon_button(ui, theme::ActionIcon::Save, "WAVに保存", theme::ButtonKind::Normal).clicked() { action = Some((index, true)); }
                                });
                            }
                        });
                        if let Some((index, save)) = action {
                            let segment = &self.segments[index];
                            self.selected = Some(index);
                            self.start = segment.start_ms as f64 / 1000.0;
                            self.end = segment.end_ms as f64 / 1000.0;
                            let output = save.then(|| (root.to_owned(), segment.text.clone()));
                            self.run_operation(output);
                        }
                    }

                });
                if busy {
                    theme::progress_message(ui, "処理中…");
                }
if let Some(error) = &self.error {
                    theme::state_message(ui, theme::State::Error, error);
                }
            });
    }
    fn save_all(&mut self, root: PathBuf) {
        let Some(input) = self.input.clone() else {
            return;
        };
        let segments = self.segments.clone();
        let (tx, rx) = mpsc::channel();
        self.operation = Some(rx);
        self.error = None;
        self.saved = None;
        std::thread::spawn(move || {
            let mut last = None;
            for (index, segment) in segments.iter().enumerate() {
                match splitter::save_material(
                    &input,
                    &root,
                    &segment.text,
                    segment.start_ms as f64 / 1000.0,
                    segment.end_ms as f64 / 1000.0,
                ) {
                    Ok(path) => last = Some(path),
                    Err(error) => {
                        let _ = tx.send(Err(format!("{}件保存後に失敗しました: {error}", index)));
                        return;
                    }
                }
            }
            let _ = tx.send(Ok(last));
        });
    }
    fn run_operation(&mut self, output: Option<(PathBuf, String)>) {
        let Some(input) = self.input.clone() else {
            return;
        };
        let (start, end) = (self.start, self.end);
        if !start.is_finite()
            || !end.is_finite()
            || start < 0.0
            || end <= start
            || end > self.duration
        {
            self.error = Some("開始と終了を動画の長さの範囲内で指定してください。".into());
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.operation = Some(rx);
        self.error = None;
        self.saved = None;
        std::thread::spawn(move || {
            let result = if let Some((root, text)) = output {
                splitter::save_material(&input, &root, &text, start, end).map(Some)
            } else {
                tempfile::tempdir()
                    .map_err(|e| e.to_string())
                    .and_then(|dir| {
                        let wav = dir.path().join("preview.wav");
                        splitter::cut(&input, &wav, start, end)?;
                        play(&wav).map(|()| None)
                    })
            };
            let _ = tx.send(result);
        });
    }
}

fn valid_segments(segments: Vec<AnalysisCandidate>, duration: f64) -> Vec<AnalysisCandidate> {
    let max_ms = (duration * 1000.0).round() as u64;
    let mut result: Vec<AnalysisCandidate> = Vec::new();
    let mut prefix = String::new();
    for mut segment in segments {
        if segment.text.trim().is_empty() {
            continue;
        }
        segment.start_ms = segment.start_ms.min(max_ms);
        segment.end_ms = segment.end_ms.min(max_ms);
        if segment.end_ms <= segment.start_ms {
            if let Some(previous) = result.last_mut() {
                previous.text.push_str(&segment.text);
            } else {
                prefix.push_str(&segment.text);
            }
            continue;
        }
        if !prefix.is_empty() {
            segment.text = format!("{prefix}{}", segment.text);
            prefix.clear();
        }
        result.push(segment);
    }
    result
}

fn results_scroll_area(reset: bool) -> egui::ScrollArea {
    let area = egui::ScrollArea::vertical()
        .id_salt("splitter_segments_scroll")
        .max_height(180.0);
    if reset {
        area.vertical_scroll_offset(0.0)
    } else {
        area
    }
}

fn segment_label(ui: &mut egui::Ui, text: &str, selected: bool, width: f32) -> egui::Response {
    ui.add_sized(
        [width, 28.0],
        egui::Button::selectable(selected, text).truncate(),
    )
    .on_hover_text(text)
}

fn range_slider(
    ui: &mut egui::Ui,
    index: usize,
    duration: f64,
    start: &mut f64,
    end: &mut f64,
) -> bool {
    let width = (ui.available_width() - 106.0).max(20.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click_and_drag());
    if duration <= 0.0 {
        return false;
    }
    let left = rect.left() + 7.0;
    let right = rect.right() - 7.0;
    let x = |t: f64| egui::lerp(left..=right, (t / duration).clamp(0.0, 1.0) as f32);
    let start_x = x(*start);
    let end_x = x(*end);
    let id = ui.id().with(("range_handle", index));
    let mut changed = false;
    if response.drag_started() || response.clicked() {
        let origin = ui.input(|input| input.pointer.press_origin());
        let handle = origin.and_then(|pos| {
            let start_distance = (pos.x - start_x).abs();
            let end_distance = (pos.x - end_x).abs();
            if start_distance.min(end_distance) <= 10.0 {
                Some(start_distance <= end_distance)
            } else {
                None
            }
        });
        ui.ctx().data_mut(|data| data.insert_temp(id, handle));
    }
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
        && let Some(is_start) = ui
            .ctx()
            .data(|data| data.get_temp::<Option<bool>>(id))
            .flatten()
    {
        let value = ((pos.x - left) / (right - left)).clamp(0.0, 1.0) as f64 * duration;
        if is_start {
            *start = value.min((*end - 0.001).max(0.0));
        } else {
            *end = value.max((*start + 0.001).min(duration));
        }
        changed = true;
    }
    let painter = ui.painter();
    let y = rect.center().y;
    painter.line_segment(
        [egui::pos2(left, y), egui::pos2(right, y)],
        egui::Stroke::new(3.0_f32, theme::state_color(theme::State::Disabled)),
    );
    painter.line_segment(
        [egui::pos2(x(*start), y), egui::pos2(x(*end), y)],
        egui::Stroke::new(5.0_f32, theme::ACCENT),
    );
    for position in [x(*start), x(*end)] {
        painter.rect_filled(
            egui::Rect::from_center_size(egui::pos2(position, y), egui::vec2(9.0, 18.0)),
            egui::CornerRadius::same(2),
            theme::ACCENT,
        );
    }
    response.on_hover_text(format!(
        "開始 {:.2} 秒 / 終了 {:.2} 秒。両端をドラッグ",
        *start, *end
    ));
    changed
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

    #[test]
    fn zero_length_tokens_keep_their_text_in_the_neighbouring_clip() {
        use crate::domain::valueobject::catalog::AnalysisCandidate;
        let candidate = |text: &str, start_ms, end_ms| AnalysisCandidate {
            text: text.into(),
            original_text: None,
            language: None,
            start_ms,
            end_ms,
            model: String::new(),
            model_version: String::new(),
            confidence: None,
            evidence: None,
        };
        let segments = super::valid_segments(
            vec![
                candidate("テ", 0, 0),
                candidate("ッカマ", 0, 900),
                candidate("ン", 900, 900),
                candidate(" ", 900, 1000),
            ],
            1.0,
        );
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "テッカマン");
        assert_eq!((segments[0].start_ms, segments[0].end_ms), (0, 900));
    }

    #[test]
    fn new_results_reset_previously_scrolled_list() {
        use eframe::egui::{self, RawInput};
        fn frame(ctx: &egui::Context, reset: bool, forced: Option<f32>) -> f32 {
            let mut offset = 0.0;
            let _ = ctx.run(RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let mut area = super::results_scroll_area(reset);
                    if let Some(value) = forced {
                        area = area.vertical_scroll_offset(value);
                    }
                    offset = area
                        .show(ui, |ui| {
                            for index in 0..40 {
                                ui.label(format!("row {index}"));
                            }
                        })
                        .state
                        .offset
                        .y;
                });
            });
            offset
        }
        let ctx = egui::Context::default();
        frame(&ctx, false, None);
        assert!(frame(&ctx, false, Some(150.0)) > 0.0);
        assert_eq!(frame(&ctx, true, None), 0.0);
        assert!(frame(&ctx, false, Some(100.0)) > 0.0);
        assert!(frame(&ctx, false, None) > 0.0);
    }

    #[test]
    fn range_columns_align_for_short_and_long_labels() {
        let context = eframe::egui::Context::default();
        super::theme::install(&context);
        let mut left_edges = Vec::new();
        let _ = context.run(eframe::egui::RawInput::default(), |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(500.0);
                let width = 150.0;
                for text in [
                    "短い",
                    "とても長い文字起こしの文章を表示して幅が変わらないようにする",
                ] {
                    ui.horizontal(|ui| {
                        super::segment_label(ui, text, false, width);
                        left_edges.push(ui.next_widget_position().x);
                    });
                }
            });
        });
        assert_eq!(left_edges.len(), 2);
        assert_eq!(left_edges[0], left_edges[1]);
    }

    #[test]
    fn save_icon_does_not_change_range_and_handles_still_drag() {
        use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput};
        fn frame(
            ctx: &egui::Context,
            events: Vec<Event>,
            start: &mut f64,
            end: &mut f64,
        ) -> (Pos2, Pos2, bool) {
            let mut save = Pos2::ZERO;
            let mut handle = Pos2::ZERO;
            let mut clicked = false;
            let _ = ctx.run(
                RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add_sized([150.0, 28.0], egui::Label::new("sample"));
                                let left = ui.next_widget_position();
                                let width = (ui.available_width() - 106.0).max(20.0);
                                handle = egui::pos2(
                                    left.x + 7.0 + (width - 14.0) * (*start as f32 / 100.0),
                                    left.y + 14.0,
                                );
                                super::range_slider(ui, 0, 100.0, start, end);
                                super::theme::icon_button(
                                    ui,
                                    super::theme::ActionIcon::Play,
                                    "play",
                                    super::theme::ButtonKind::Normal,
                                );
                                let response = super::theme::icon_button(
                                    ui,
                                    super::theme::ActionIcon::Save,
                                    "save",
                                    super::theme::ButtonKind::Normal,
                                );
                                save = response.rect.center();
                                clicked = response.clicked();
                            });
                        });
                    });
                },
            );
            (save, handle, clicked)
        }
        fn pointer(pos: Pos2, pressed: bool) -> Event {
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::default(),
            }
        }
        let ctx = egui::Context::default();
        super::theme::install(&ctx);
        let (mut start, mut end) = (20.0, 40.0);
        let (save, _, _) = frame(&ctx, vec![], &mut start, &mut end);
        frame(
            &ctx,
            vec![Event::PointerMoved(save), pointer(save, true)],
            &mut start,
            &mut end,
        );
        let (_, handle, clicked) = frame(&ctx, vec![pointer(save, false)], &mut start, &mut end);
        assert!(clicked);
        assert_eq!((start, end), (20.0, 40.0));
        frame(
            &ctx,
            vec![Event::PointerMoved(handle), pointer(handle, true)],
            &mut start,
            &mut end,
        );
        let moved = handle + egui::vec2(20.0, 0.0);
        frame(&ctx, vec![Event::PointerMoved(moved)], &mut start, &mut end);
        frame(&ctx, vec![pointer(moved, false)], &mut start, &mut end);
        assert!(start > 20.0 && start < end);
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
