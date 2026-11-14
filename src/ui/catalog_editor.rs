use crate::infra::local_ai::{
    local_stt::{self, SttConfig},
    ollama,
};
use crate::{
    domain::valueobject::catalog::*,
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
    ui::theme,
};
use eframe::egui::{self, Ui};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorTab {
    Search,
    Metadata,
    Analysis,
}

pub(super) const PANEL_WIDTH: f32 = 640.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PanelEvent {
    None,
    Saved,
    Closed,
}

pub(super) struct CatalogEditor {
    pub store: SqliteCatalog,
    pub open: bool,
    materials: Vec<Material>,
    stored_materials: Vec<StoredMaterial>,
    material_id: Option<i64>,
    keywords: String,
    tags: String,
    roles: String,
    note: String,
    query: String,
    tag_filter: String,
    role_filter: String,
    active_tab: EditorTab,
    keep_open: bool,
    message: Option<(theme::State, String)>,
    ffmpeg_path: String,
    whisper_path: String,
    model_path: String,
    ollama_model: String,
    ollama_models: Vec<String>,
    ollama_models_task: Option<Receiver<Result<Vec<String>, ollama::OllamaError>>>,
    ollama_available: Option<bool>,
    ollama_start_requested: bool,
    audio_conversion: bool,
    speech_recognition: bool,
    force_reanalysis: bool,
    analysis_task: Option<Receiver<(i64, AnalysisRecord)>>,
    analysis_cancel: Option<Arc<AtomicBool>>,
    analysis_running_id: Option<i64>,
}

impl CatalogEditor {
    pub fn new(store: SqliteCatalog) -> Result<Self, CatalogError> {
        let materials = store.materials()?;
        let stored_materials = store.search(&SearchFilter::default())?;
        let material_id = materials.first().map(|material| material.id);
        let (models_sender, models_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = models_sender.send(ollama::local_models());
        });
        let mut editor = Self {
            store,
            open: true,
            materials,
            stored_materials,
            material_id,
            keywords: String::new(),
            tags: String::new(),
            roles: String::new(),
            note: String::new(),
            query: String::new(),
            tag_filter: String::new(),
            role_filter: String::new(),
            active_tab: EditorTab::Search,
            keep_open: false,
            message: None,
            ffmpeg_path: "ffmpeg".into(),
            whisper_path: default_whisper_path("Release/whisper-cli.exe"),
            model_path: default_whisper_path("ggml-tiny.bin"),
            ollama_model: String::new(),
            ollama_models: Vec::new(),
            ollama_models_task: Some(models_receiver),
            ollama_available: None,
            ollama_start_requested: false,
            audio_conversion: false,
            speech_recognition: false,
            force_reanalysis: false,
            analysis_task: None,
            analysis_cancel: None,
            analysis_running_id: None,
        };
        editor.load_selected();
        Ok(editor)
    }

    pub fn count(&self) -> usize {
        self.materials.len()
    }

    pub fn show_panel(&mut self, ui: &mut Ui, enabled: bool) -> PanelEvent {
        let mut event = PanelEvent::None;
        theme::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::section_title("素材の情報を登録"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled_ui(self.analysis_task.is_none(), |ui| {
                            theme::icon_button(
                                ui,
                                theme::ActionIcon::Close,
                                "閉じる（01へ戻る）",
                                theme::ButtonKind::Normal,
                            )
                        })
                        .inner
                        .clicked()
                    {
                        self.open = false;
                        event = PanelEvent::Closed;
                    }
                });
            });
            ui.separator();
            if event == PanelEvent::None {
                ui.add_enabled_ui(enabled, |ui| {
                    if self.controls(ui) {
                        event = PanelEvent::Saved;
                    }
                });
            }
        });
        event
    }

    fn controls(&mut self, ui: &mut Ui) -> bool {
        if let Some(task) = &self.analysis_task {
            match task.try_recv() {
                Ok((id, record)) => {
                    let state = if record.status == AnalysisStatus::Completed {
                        theme::State::Ready
                    } else {
                        theme::State::Attention
                    };
                    let message = match record.status {
                        AnalysisStatus::Completed => {
                            "解析結果を保存しました。解析タブで確認してください。"
                        }
                        AnalysisStatus::Interrupted => "解析を中断しました。",
                        _ => "解析に失敗しました。解析タブで理由を確認してください。",
                    };
                    self.message = match self.store.save_analysis(id, &record) {
                        Ok(()) => Some((state, message.into())),
                        Err(e) => Some((theme::State::Error, e.to_string())),
                    };
                    self.analysis_task = None;
                    self.analysis_cancel = None;
                    self.analysis_running_id = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.message = Some((theme::State::Error, "解析処理が中断されました。".into()));
                    self.analysis_task = None;
                    self.analysis_cancel = None;
                    self.analysis_running_id = None;
                }
                Err(TryRecvError::Empty) => ui
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        ui.label(theme::body("素材に入っている言葉・タグ・役割を手で登録できます。").strong());
        ui.label(theme::body(
            "開始・終了時刻は入力しません。素材ファイル1本が、そのまま候補の単位です。",
        ));
        if self.materials.is_empty() {
            ui.separator();
            ui.label(theme::body("まだ登録できる素材がありません。").strong());
            ui.label(theme::body(
                "1. 選んだ素材フォルダに動画または音声ファイルを入れます。",
            ));
            ui.label(theme::body(
                "2. この画面を閉じ、01 / MATERIAL CATALOG の更新アイコンを押します。",
            ));
            return false;
        }
        ui.horizontal(|ui| {
            Self::tab_button(ui, &mut self.active_tab, EditorTab::Search, "素材を探す");
            Self::tab_button(ui, &mut self.active_tab, EditorTab::Metadata, "手入力");
            Self::tab_button(ui, &mut self.active_tab, EditorTab::Analysis, "解析");
        });
        ui.add_space(theme::SPACE_SM);
        match self.active_tab {
            EditorTab::Search => self.search_controls(ui),
            EditorTab::Metadata => self.metadata_controls(ui),
            EditorTab::Analysis => self.analysis_controls(ui),
        }
    }

    fn analysis_controls(&mut self, ui: &mut Ui) -> bool {
        ui.label(theme::body(
            "選択中の素材をローカルで文字起こしします。結果は手入力の情報と別に保存します。",
        ));
        ui.label(theme::body(
            "再解析が成功すると、以前の解析候補とその修正は置き換わります。",
        ));
        let previous = self.material_id;
        ui.horizontal(|ui| {
            theme::form_label(ui, "素材");
            egui::ComboBox::from_id_salt("analysis_material")
                .selected_text(
                    self.materials
                        .iter()
                        .find(|m| Some(m.id) == self.material_id)
                        .map_or("素材なし", |m| m.source_path.as_str()),
                )
                .show_ui(ui, |ui| {
                    for material in &self.materials {
                        ui.selectable_value(
                            &mut self.material_id,
                            Some(material.id),
                            &material.source_path,
                        );
                    }
                });
        });
        if previous != self.material_id {
            self.load_selected();
        }
        ui.horizontal(|ui| {
            theme::form_label(ui, "音声変換");
            egui::ComboBox::from_id_salt("audio_converter")
                .selected_text(if self.audio_conversion {
                    "FFmpeg"
                } else {
                    "なし"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.audio_conversion, false, "なし");
                    ui.selectable_value(&mut self.audio_conversion, true, "FFmpeg");
                });
        });
        ui.horizontal(|ui| {
            theme::form_label(ui, "音声認識");
            egui::ComboBox::from_id_salt("speech_recognizer")
                .selected_text(if self.speech_recognition {
                    "Whisper"
                } else {
                    "なし"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.speech_recognition, false, "なし");
                    ui.selectable_value(&mut self.speech_recognition, true, "Whisper");
                });
            if self.speech_recognition {
                theme::local_label(ui);
            }
        });
        if let Some(task) = &self.ollama_models_task {
            match task.try_recv() {
                Ok(Ok(models)) => {
                    self.ollama_models = models;
                    self.ollama_available = Some(true);
                    self.ollama_start_requested = false;
                    self.ollama_models_task = None;
                }
                Ok(Err(error)) => {
                    self.ollama_models.clear();
                    self.ollama_available = Some(false);
                    if self.ollama_start_requested {
                        self.message = Some((theme::State::Error, error.to_string()));
                    }
                    self.ollama_start_requested = false;
                    self.ollama_models_task = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.ollama_models_task = None;
                }
                Err(TryRecvError::Empty) => ui
                    .ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200)),
            }
        }
        ui.horizontal(|ui| {
            theme::form_label(ui, "テキスト解析");
            egui::ComboBox::from_id_salt("text_analyzer")
                .selected_text(if self.ollama_model.is_empty() {
                    "なし"
                } else {
                    "Ollama"
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.ollama_model.is_empty(), "なし")
                        .clicked()
                    {
                        self.ollama_model.clear();
                    }
                    for model in &self.ollama_models {
                        if ui
                            .selectable_label(
                                self.ollama_model == *model,
                                format!("Ollama / {model}"),
                            )
                            .clicked()
                        {
                            self.ollama_model.clone_from(model);
                        }
                    }
                });
            if theme::button(ui, "再読み込み", theme::ButtonKind::Normal)
                .on_hover_text("利用できるモデルの一覧を再取得")
                .clicked()
                && self.ollama_models_task.is_none()
            {
                let (sender, receiver) = mpsc::channel();
                self.ollama_models_task = Some(receiver);
                self.ollama_start_requested = false;
                std::thread::spawn(move || {
                    let _ = sender.send(ollama::local_models());
                });
            }
            if !self.ollama_model.is_empty() {
                ui.label(theme::detail(&self.ollama_model));
                theme::local_label(ui);
            }
        });
        if self.ollama_available == Some(false) {
            ui.label(theme::body(
                "Ollama に接続できません。使わない場合は「なし」で続けられます。",
            ));
            if ui
                .add_enabled_ui(self.ollama_models_task.is_none(), |ui| {
                    theme::button(ui, "Ollama を起動", theme::ButtonKind::Normal)
                })
                .inner
                .clicked()
            {
                let (sender, receiver) = mpsc::channel();
                self.ollama_models_task = Some(receiver);
                self.ollama_start_requested = true;
                std::thread::spawn(move || {
                    let _ = sender.send(ollama::start_and_list_models());
                });
            }
        } else if self.ollama_available == Some(true) && self.ollama_models.is_empty() {
            ui.label(theme::body(
                "Ollama にローカルモデルがありません。「なし」で文字起こしできます。",
            ));
        }
        ui.collapsing("実行ファイルとモデルの設定", |ui| {
            for (label, value) in [
                ("音声変換", &mut self.ffmpeg_path),
                ("音声認識", &mut self.whisper_path),
                ("音声認識モデル", &mut self.model_path),
            ] {
                ui.horizontal(|ui| {
                    ui.label(theme::detail(label));
                    ui.add_sized(
                        [250.0, 22.0],
                        egui::Label::new(theme::detail_muted(if value.is_empty() {
                            "未選択"
                        } else {
                            value.as_str()
                        }))
                        .truncate(),
                    );
                    if theme::button(ui, "選択", theme::ButtonKind::Normal).clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_file()
                    {
                        *value = path.to_string_lossy().into_owned();
                    }
                });
            }
        });
        if self.audio_conversion
            && self.speech_recognition
            && (!executable_exists(&self.ffmpeg_path)
                || !executable_exists(&self.whisper_path)
                || !PathBuf::from(&self.model_path).is_file())
        {
            ui.label(theme::body("必要な実行ファイルまたは音声認識モデルが見つかりません。「実行ファイルとモデルの設定」から選択してください。"));
        }
        if self.analysis_task.is_some() {
            ui.label(theme::body("解析中…"));
            ui.label(theme::detail_muted(
                "Ollama 実行中の中断は応答を待って反映されます。",
            ));
            if theme::button(ui, "解析を中断", theme::ButtonKind::Normal).clicked()
                && let Some(cancel) = &self.analysis_cancel
            {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        ui.checkbox(&mut self.force_reanalysis, "同じ素材と設定でも再解析する");
        if ui
            .add_enabled_ui(
                self.analysis_task.is_none()
                    && self.material_id.is_some()
                    && self.audio_conversion
                    && self.speech_recognition
                    && executable_exists(&self.ffmpeg_path)
                    && executable_exists(&self.whisper_path)
                    && PathBuf::from(&self.model_path).is_file(),
                |ui| theme::button(ui, "文字起こしを開始", theme::ButtonKind::Primary),
            )
            .inner
            .clicked()
            && let Some(material) = self
                .materials
                .iter()
                .find(|m| Some(m.id) == self.material_id)
        {
            let id = material.id;
            let path = self.store.root().join(&material.source_path);
            let root = self.store.root().join("materials");
            let safe = path
                .canonicalize()
                .ok()
                .zip(root.canonicalize().ok())
                .filter(|(path, root)| path.starts_with(root));
            if let Some((path, _)) = safe {
                let config = SttConfig {
                    ffmpeg: PathBuf::from(&self.ffmpeg_path),
                    whisper: PathBuf::from(&self.whisper_path),
                    model: PathBuf::from(&self.model_path),
                };
                let ollama_model = self.ollama_model.trim().to_owned();
                let fingerprint = material.fingerprint.clone();
                let configuration = format!("{} / {}", config.model.display(), ollama_model);
                let previous = match self.store.analysis(id) {
                    Ok(record) => record,
                    Err(error) => {
                        self.message = Some((theme::State::Error, error.to_string()));
                        self.show_message(ui);
                        return false;
                    }
                };
                if !self.force_reanalysis
                    && previous.as_ref().is_some_and(|record| {
                        record.status == AnalysisStatus::Completed
                            && record.fingerprint == fingerprint
                            && record.configuration == configuration
                    })
                {
                    self.message = Some((theme::State::Ready, "同じ素材と設定の解析結果を表示しています。再実行には「同じ素材と設定でも再解析する」を選んでください。".into()));
                    self.show_message(ui);
                    return false;
                }
                let had_previous = previous.is_some();
                let mut record = previous.unwrap_or_else(|| AnalysisRecord {
                    fingerprint: fingerprint.clone(),
                    configuration: format!("{}", config.model.display()),
                    status: AnalysisStatus::Running,
                    candidates: Vec::new(),
                    error: None,
                });
                record.status = AnalysisStatus::Running;
                record.error = None;
                match self.store.save_analysis(id, &record) {
                    Ok(()) => {
                        let (sender, receiver) = mpsc::channel();
                        self.analysis_task = Some(receiver);
                        self.analysis_running_id = Some(id);
                        let cancel = Arc::new(AtomicBool::new(false));
                        self.analysis_cancel = Some(Arc::clone(&cancel));
                        std::thread::spawn(move || {
                            match local_stt::transcribe(&config, &path, &fingerprint, &cancel) {
                                Ok(mut candidates) => {
                                    if !ollama_model.is_empty()
                                        && !candidates.is_empty()
                                        && !cancel.load(Ordering::Relaxed)
                                    {
                                        match ollama::summarize(&ollama_model, &candidates) {
                                            Ok(summary) => candidates.push(summary),
                                            Err(error) => {
                                                record.error = Some(error.to_string());
                                                record.status = if cancel.load(Ordering::Relaxed) {
                                                    AnalysisStatus::Interrupted
                                                } else {
                                                    AnalysisStatus::Failed
                                                };
                                                if !had_previous {
                                                    record.fingerprint = fingerprint;
                                                    record.configuration = configuration;
                                                    record.candidates = candidates;
                                                }
                                                let _ = sender.send((id, record));
                                                return;
                                            }
                                        }
                                    }
                                    if cancel.load(Ordering::Relaxed) {
                                        record.status = AnalysisStatus::Interrupted;
                                    } else {
                                        record.fingerprint = fingerprint;
                                        record.configuration = configuration;
                                        record.candidates = candidates;
                                        record.status = AnalysisStatus::Completed;
                                    }
                                }
                                Err(error) => {
                                    record.error = Some(error.to_string());
                                    record.status =
                                        if matches!(error, local_stt::SttError::Interrupted) {
                                            AnalysisStatus::Interrupted
                                        } else {
                                            AnalysisStatus::Failed
                                        };
                                }
                            }
                            let _ = sender.send((id, record));
                        });
                    }
                    Err(e) => self.message = Some((theme::State::Error, e.to_string())),
                }
            } else {
                self.message = Some((
                    theme::State::Error,
                    "素材を読み取れません。基準フォルダを開き直してください。".into(),
                ));
            }
        }
        if let Some(id) = self.material_id {
            match self.store.analysis(id) {
                Ok(Some(mut record)) => {
                    if record.status == AnalysisStatus::Running && self.analysis_task.is_none() {
                        ui.label(theme::body(
                            "前回の解析は完了していません。必要なら再解析してください。",
                        ));
                    }
                    if let Some(error) = &record.error {
                        ui.label(theme::body(error));
                    }
                    if self
                        .materials
                        .iter()
                        .find(|m| m.id == id)
                        .is_some_and(|m| m.fingerprint != record.fingerprint)
                    {
                        ui.label(theme::body("素材が変更されています。再解析してください。"));
                    }
                    if !record.candidates.is_empty() {
                        ui.separator();
                        ui.label(theme::section_title("保存済みの解析結果"));
                    }
                    let mut changed = false;
                    for candidate in &mut record.candidates {
                        if let Some(evidence) = &candidate.evidence {
                            ui.label(theme::detail(evidence));
                        }
                        ui.label(theme::detail(format!(
                            "素材の先頭から {}.{:03}秒〜{}.{:03}秒",
                            candidate.start_ms / 1000,
                            candidate.start_ms % 1000,
                            candidate.end_ms / 1000,
                            candidate.end_ms % 1000
                        )));
                        ui.label(theme::detail(format!("使用モデル: {}", candidate.model)));
                        if let Some(language) = &candidate.language {
                            ui.label(theme::detail(format!("言語候補: {language}")));
                        }
                        if let Some(original) = &candidate.original_text
                            && original != &candidate.text
                        {
                            ui.label(theme::detail_muted(format!("生成時: {original}")));
                        }
                        changed |= ui
                            .add_enabled(
                                self.analysis_task.is_none(),
                                egui::TextEdit::multiline(&mut candidate.text),
                            )
                            .changed();
                    }
                    if changed && let Err(e) = self.store.save_analysis(id, &record) {
                        self.message = Some((theme::State::Error, e.to_string()));
                    }
                }
                Err(e) => self.message = Some((theme::State::Error, e.to_string())),
                _ => {}
            }
        }
        self.show_message(ui);
        false
    }

    fn tab_button(ui: &mut Ui, active_tab: &mut EditorTab, tab: EditorTab, label: &str) {
        let selected = *active_tab == tab;
        if theme::tab(ui, label, selected, 184.0).clicked() {
            *active_tab = tab;
        }
    }

    fn search_controls(&mut self, ui: &mut Ui) -> bool {
        ui.label(theme::body(
            "素材名や含まれる言葉を入力すると、候補を絞り込みます。選ぶと素材情報の登録画面を開きます。",
        ));
        let mut filters_changed = false;
        ui.horizontal(|ui| {
            ui.label(theme::detail("言葉・素材名"));
            let response = theme::text_input(ui, &mut self.query, "例: テッカマン", 280.0);
            filters_changed |= response.changed();
        });
        ui.collapsing("詳細検索（タグ・役割）", |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::detail("タグで絞り込み"));
                filters_changed |= theme::text_input(ui, &mut self.tag_filter, "", 130.0).changed();
                ui.label(theme::detail("役割で絞り込み"));
                filters_changed |=
                    theme::text_input(ui, &mut self.role_filter, "", 130.0).changed();
            });
        });
        if filters_changed {
            match self.refresh() {
                Ok(()) => self.message = None,
                Err(error) => self.message = Some((theme::State::Error, error.to_string())),
            }
        }
        let has_filter = !self.query.trim().is_empty()
            || !self.tag_filter.trim().is_empty()
            || !self.role_filter.trim().is_empty();
        if !has_filter {
            ui.label(theme::body_muted("入力すると候補を表示します。"));
            return false;
        }
        ui.label(theme::detail_muted(
            "要確認の素材も表示します。GUIDEの候補には使いません。",
        ));
        let mut selection = None;
        theme::suggestion_list(
            ui,
            "stored_materials",
            theme::SUGGESTION_LIST_HEIGHT,
            |ui| {
                if self.stored_materials.is_empty() {
                    ui.label(theme::body_muted("一致する素材はありません。"));
                }
                for material in &self.stored_materials {
                    let keywords = material.metadata.keywords.join(", ");
                    let label = if keywords.is_empty() {
                        material.source_path.clone()
                    } else {
                        format!("{} / {keywords}", material.source_path)
                    };
                    let state = (material.availability == Availability::Stale)
                        .then_some(("要確認", theme::State::Attention));
                    if theme::suggestion_item(ui, &label, state).clicked() {
                        selection = Some(material.id);
                    }
                }
            },
        );
        if let Some(id) = selection {
            self.material_id = Some(id);
            self.load_selected();
            self.active_tab = EditorTab::Metadata;
        }
        self.show_message(ui);
        false
    }

    fn metadata_controls(&mut self, ui: &mut Ui) -> bool {
        ui.label(theme::body(
            "検索語句・タグ・役割は、この素材に保存する情報です。",
        ));
        let selected = self.material_id;
        egui::ComboBox::from_id_salt("material")
            .width(640.0)
            .selected_text(
                self.materials
                    .iter()
                    .find(|material| Some(material.id) == self.material_id)
                    .map_or("素材なし", |material| material.source_path.as_str()),
            )
            .show_ui(ui, |ui| {
                for material in &self.materials {
                    ui.selectable_value(
                        &mut self.material_id,
                        Some(material.id),
                        &material.source_path,
                    );
                }
            });
        if selected != self.material_id {
            self.load_selected();
        }
        ui.separator();
        egui::Grid::new("metadata").num_columns(2).show(ui, |ui| {
            for (label, value) in [
                ("検索語句", &mut self.keywords),
                ("タグ", &mut self.tags),
                ("制作上の役割", &mut self.roles),
                ("メモ", &mut self.note),
            ] {
                ui.label(theme::detail(label));
                theme::text_input(ui, value, "", 510.0);
                ui.end_row();
            }
        });
        ui.label(theme::detail_muted(
            "複数の検索語句・タグ・役割はカンマ区切りです。",
        ));
        let selected = self.material_id.is_some();
        let mut changed = false;
        ui.checkbox(&mut self.keep_open, "連続で登録する");
        let save_label = if self.keep_open {
            "保存して続ける"
        } else {
            "保存して閉じる"
        };
        if ui
            .add_enabled_ui(selected, |ui| {
                theme::button(ui, save_label, theme::ButtonKind::Primary)
            })
            .inner
            .clicked()
        {
            match self.save() {
                Ok(()) => {
                    self.message = if self.keep_open {
                        Some((
                            theme::State::Ready,
                            "素材の情報を保存しました。続けて別の素材を登録できます。".into(),
                        ))
                    } else {
                        self.open = false;
                        None
                    };
                    changed = true;
                }
                Err(error) => self.message = Some((theme::State::Error, error.to_string())),
            }
        }
        self.show_message(ui);
        changed
    }

    fn load_selected(&mut self) {
        let Some(id) = self.material_id else {
            return;
        };
        let metadata = self
            .stored_materials
            .iter()
            .find(|material| material.id == id)
            .map(|material| &material.metadata)
            .cloned()
            .unwrap_or_default();
        self.keywords = metadata.keywords.join(", ");
        self.tags = metadata.tags.join(", ");
        self.roles = metadata.roles.join(", ");
        self.note = metadata.note;
        self.message = None;
    }

    fn show_message(&self, ui: &mut Ui) {
        if let Some((state, message)) = &self.message {
            theme::state_message(ui, *state, message);
        }
    }

    fn save(&mut self) -> Result<(), CatalogError> {
        let material_id = self.material_id.ok_or(CatalogError::NoMaterialSelected)?;
        let metadata = Metadata {
            keywords: split(&self.keywords),
            tags: split(&self.tags),
            roles: split(&self.roles),
            note: self.note.clone(),
        };
        self.store.save_metadata(material_id, &metadata)?;
        self.refresh()?;
        self.load_selected();
        Ok(())
    }

    fn refresh(&mut self) -> Result<(), CatalogError> {
        self.materials = self.store.materials()?;
        self.stored_materials = self.store.search(&SearchFilter {
            text: &self.query,
            tag: nonempty(&self.tag_filter),
            role: nonempty(&self.role_filter),
        })?;
        Ok(())
    }
}

impl Drop for CatalogEditor {
    fn drop(&mut self) {
        if let Some(cancel) = &self.analysis_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
        if let Some(id) = self.analysis_running_id
            && let Ok(Some(mut record)) = self.store.analysis(id)
        {
            record.status = AnalysisStatus::Interrupted;
            record.error = Some("アプリ終了により解析を中断しました".into());
            let _ = self.store.save_analysis(id, &record);
        }
    }
}

fn split(text: &str) -> Vec<String> {
    text.split([',', '、'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn default_whisper_path(relative: &str) -> String {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("cutup-studio/whisper").join(relative))
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn executable_exists(value: &str) -> bool {
    let path = std::path::Path::new(value);
    if value.is_empty() {
        return false;
    }
    if path.is_file() {
        return true;
    }
    if path.components().count() != 1 {
        return false;
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(value);
            candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
        })
    })
}

fn nonempty(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::service::create_plan;

    #[test]
    fn editor_saved_material_metadata_survives_reopening_and_produces_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("sample.wav"), b"local media fixture").unwrap();
        let path = temp.path().join("catalog.sqlite");
        let mut store = SqliteCatalog::create(&path, temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::new(store).unwrap();
        editor.keywords = "テッカマン".into();
        editor.tags = "画, 反応".into();
        editor.roles = "導入".into();
        editor.save().unwrap();
        let context = egui::Context::default();
        let output = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.show_panel(ui, true);
            });
        });
        assert!(!output.shapes.is_empty());
        drop(editor);
        let reopened = CatalogEditor::new(SqliteCatalog::open(&path).unwrap()).unwrap();
        assert_eq!(
            reopened.stored_materials[0].metadata.tags,
            vec!["画", "反応"]
        );
        let catalog = reopened.store.candidate_catalog().unwrap();
        let plan = create_plan(&catalog, "導入: テッカマン");
        assert_eq!(plan.slots[0].candidates.len(), 1);
        assert_eq!(plan.slots[0].candidates[0].tags, vec!["画", "反応"]);
    }
}
