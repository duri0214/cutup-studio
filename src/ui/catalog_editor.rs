use crate::infra::local_ai::{
    local_stt::{self, SpeechLanguage, SttConfig},
    ollama,
};
use crate::{
    domain::valueobject::catalog::*,
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
    ui::theme,
};
use eframe::egui::{self, Ui};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
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
type AnalysisTask = Receiver<(
    i64,
    AnalysisRecord,
    String,
    Option<ollama::MetadataSuggestions>,
)>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct EditorSettings {
    whisper_path: String,
    model_path: String,
    ollama_model: String,
    speech_recognition: bool,
    speech_language: SpeechLanguage,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            whisper_path: default_whisper_path("Release/whisper-cli.exe"),
            model_path: default_whisper_path("ggml-tiny.bin"),
            ollama_model: String::new(),
            speech_recognition: false,
            speech_language: SpeechLanguage::Auto,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PanelEvent {
    None,
    Saved,
    Closed,
}

enum OllamaTaskResult {
    Refreshed(Vec<String>),
    Started(ollama::StartOutcome),
}

pub(super) struct CatalogEditor {
    pub store: SqliteCatalog,
    pub open: bool,
    materials: Vec<Material>,
    stored_materials: Vec<StoredMaterial>,
    material_id: Option<i64>,
    drafts: HashMap<i64, (String, String, String, String)>,
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
    whisper_path: String,
    model_path: String,
    ollama_model: String,
    ollama_models: Vec<String>,
    ollama_models_task: Option<Receiver<Result<OllamaTaskResult, ollama::OllamaError>>>,
    ollama_available: Option<bool>,
    ollama_start_requested: bool,
    speech_recognition: bool,
    speech_language: SpeechLanguage,
    analysis_task: Option<AnalysisTask>,
    analysis_cancel: Option<Arc<AtomicBool>>,
    analysis_running_id: Option<i64>,
    confirm_close: bool,
}

impl CatalogEditor {
    pub(super) fn with_settings(
        store: SqliteCatalog,
        settings: EditorSettings,
    ) -> Result<Self, CatalogError> {
        let materials = store.materials()?;
        let stored_materials = store.search(&SearchFilter::default())?;
        let material_id = materials.first().map(|material| material.id);
        let (models_sender, models_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = models_sender.send(ollama::local_models().map(OllamaTaskResult::Refreshed));
        });
        let mut editor = Self {
            store,
            open: true,
            materials,
            stored_materials,
            material_id,
            drafts: HashMap::new(),
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
            whisper_path: settings.whisper_path,
            model_path: settings.model_path,
            ollama_model: settings.ollama_model,
            ollama_models: Vec::new(),
            ollama_models_task: Some(models_receiver),
            ollama_available: None,
            ollama_start_requested: false,
            speech_recognition: settings.speech_recognition,
            speech_language: settings.speech_language,
            analysis_task: None,
            analysis_cancel: None,
            analysis_running_id: None,
            confirm_close: false,
        };
        editor.load_selected();
        Ok(editor)
    }

    pub(super) fn settings(&self) -> EditorSettings {
        EditorSettings {
            whisper_path: self.whisper_path.clone(),
            model_path: self.model_path.clone(),
            ollama_model: self.ollama_model.clone(),
            speech_recognition: self.speech_recognition,
            speech_language: self.speech_language,
        }
    }

    pub fn selected_ollama_model(&self) -> Option<&str> {
        (!self.ollama_model.is_empty()).then_some(self.ollama_model.as_str())
    }

    pub fn count(&self) -> usize {
        self.materials.len()
    }

    pub fn message(&self) -> Option<(theme::State, &str)> {
        self.message
            .as_ref()
            .map(|(state, text)| (*state, text.as_str()))
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
                        match self.has_unsaved_metadata() {
                            Ok(true) => self.confirm_close = true,
                            Ok(false) => {
                                self.discard_changes();
                                self.open = false;
                                event = PanelEvent::Closed;
                            }
                            Err(error) => {
                                self.message = Some((theme::State::Error, error.to_string()))
                            }
                        }
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
        if self.confirm_close {
            egui::Modal::new(egui::Id::new("confirm_close_catalog_editor")).show(ui.ctx(), |ui| {
                ui.label(theme::section_title("未保存の素材情報"));
                ui.label("保存していない素材情報があります。閉じる前に保存しますか？");
                ui.horizontal(|ui| {
                    if theme::button(ui, "保存して閉じる", theme::ButtonKind::Primary).clicked()
                    {
                        match self.save_all_drafts() {
                            Ok(()) => {
                                self.confirm_close = false;
                                self.open = false;
                                event = PanelEvent::Saved;
                            }
                            Err(error) => {
                                self.message = Some((theme::State::Error, error.to_string()))
                            }
                        }
                    }
                    if theme::button(ui, "保存せず閉じる", theme::ButtonKind::Normal).clicked()
                    {
                        self.discard_changes();
                        self.confirm_close = false;
                        self.open = false;
                        event = PanelEvent::Closed;
                    }
                    if theme::button(ui, "キャンセル", theme::ButtonKind::Normal).clicked() {
                        self.confirm_close = false;
                    }
                });
            });
        }
        event
    }

    fn controls(&mut self, ui: &mut Ui) -> bool {
        if let Some(task) = &self.analysis_task {
            match task.try_recv() {
                Ok((id, record, transcript, suggestions)) => {
                    let completed = record.status == AnalysisStatus::Completed;
                    let partial_failure = record.error.is_some();
                    let state = if completed && !partial_failure {
                        theme::State::Ready
                    } else {
                        theme::State::Attention
                    };
                    let message = match (record.status, partial_failure) {
                        (AnalysisStatus::Completed, false) => {
                            "音声から情報を反映しました。編集欄を確認し、必要なら修正してください。"
                        }
                        (AnalysisStatus::Completed, true) => {
                            "文字起こしを反映しました。Ollama の分類に失敗したため、タグと役割を確認してください。"
                        }
                        (AnalysisStatus::Interrupted, _) => {
                            "情報抽出を中断しました。編集中の情報は保持しています。"
                        }
                        _ => "解析に失敗しました。編集中の情報は保持しています。",
                    };
                    self.message = match self.store.save_analysis(id, &record) {
                        Ok(()) => {
                            if completed {
                                self.apply_suggestions(id, &transcript, suggestions.as_ref());
                            }
                            Some((
                                state,
                                if let Some(error) = record.error {
                                    format!("{message} {error}")
                                } else {
                                    message.into()
                                },
                            ))
                        }
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
        ui.label(theme::body("音声から検索語句を起こし、Ollama でタグと役割を補います。結果を確認してから保存してください。").strong());
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
            Self::tab_button(ui, &mut self.active_tab, EditorTab::Metadata, "編集");
            Self::tab_button(ui, &mut self.active_tab, EditorTab::Analysis, "設定");
        });
        ui.add_space(theme::SPACE_SM);
        match self.active_tab {
            EditorTab::Search => self.search_controls(ui),
            EditorTab::Metadata => self.metadata_controls(ui),
            EditorTab::Analysis => self.settings_controls(ui),
        }
    }

    fn settings_controls(&mut self, ui: &mut Ui) -> bool {
        ui.label(theme::body("入力支援に使う方式とモデルを設定します。"));
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
            self.remember_draft(previous);
            self.load_selected();
        }
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
                ui.add_space(theme::SPACE_SM);
                theme::form_label(ui, "音声の言語");
                egui::ComboBox::from_id_salt("speech_language")
                    .selected_text(match self.speech_language {
                        SpeechLanguage::Auto => "自動検出",
                        SpeechLanguage::Japanese => "日本語",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.speech_language,
                            SpeechLanguage::Auto,
                            "自動検出",
                        );
                        ui.selectable_value(
                            &mut self.speech_language,
                            SpeechLanguage::Japanese,
                            "日本語",
                        );
                    });
            }
        });
        if let Some(task) = &self.ollama_models_task {
            match task.try_recv() {
                Ok(Ok(result)) => {
                    let (models, message) = match result {
                        OllamaTaskResult::Refreshed(models) => (models, None),
                        OllamaTaskResult::Started(ollama::StartOutcome::Started(models)) => {
                            (models, Some("Ollama を起動しました。"))
                        }
                        OllamaTaskResult::Started(ollama::StartOutcome::AlreadyRunning(models)) => {
                            (
                                models,
                                Some(
                                    "Ollama は既に起動しています。通知領域にアイコンがない場合は、既存のサーバーを停止してから再度起動してください。",
                                ),
                            )
                        }
                    };
                    if !self.ollama_model.is_empty() && !models.contains(&self.ollama_model) {
                        self.ollama_model.clear();
                    }
                    self.ollama_models = models;
                    self.ollama_available = Some(true);
                    if let Some(message) = message {
                        self.message = Some((theme::State::Ready, message.into()));
                    }
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
            let models_menu = egui::ComboBox::from_id_salt("text_analyzer")
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
                    if self.ollama_models_task.is_some() {
                        ui.label(theme::detail_muted("モデルを読み込み中…"));
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
            if models_menu.response.clicked()
                && models_menu.inner.is_some()
                && self.ollama_models_task.is_none()
            {
                let (sender, receiver) = mpsc::channel();
                self.ollama_models.clear();
                self.ollama_models_task = Some(receiver);
                self.ollama_start_requested = false;
                ui.ctx().request_repaint();
                std::thread::spawn(move || {
                    let _ = sender.send(ollama::local_models().map(OllamaTaskResult::Refreshed));
                });
            }
            let installed = ollama::is_installed();
            let tooltip = if installed {
                "Ollama を起動"
            } else {
                "Ollama が見つかりません。インストールしてください。"
            };
            if ui
                .add_enabled_ui(installed && !self.ollama_start_requested, |ui| {
                    theme::icon_button(
                        ui,
                        theme::ActionIcon::Ollama,
                        tooltip,
                        theme::ButtonKind::Normal,
                    )
                })
                .inner
                .clicked()
            {
                let (sender, receiver) = mpsc::channel();
                self.ollama_models_task = Some(receiver);
                self.ollama_start_requested = true;
                self.message = Some((theme::State::Ready, "Ollama を起動しています。".into()));
                std::thread::spawn(move || {
                    let _ =
                        sender.send(ollama::start_and_list_models().map(OllamaTaskResult::Started));
                });
            }
            if self.ollama_available == Some(true) && self.ollama_models.is_empty() {
                ui.label(theme::detail_muted("モデルなし")).on_hover_text(
                    "Ollama にローカルモデルがありません。「なし」で文字起こしできます。",
                );
            }
            if !self.ollama_model.is_empty() {
                ui.label(theme::detail(&self.ollama_model));
                theme::local_label(ui);
                if self.ollama_available == Some(false) {
                    ui.label(theme::detail_muted(
                        "Ollama は利用できません。選択は保持しています。",
                    ));
                }
            }
        });
        ui.collapsing("実行ファイルとモデルの設定", |ui| {
            for (label, value) in [
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
        self.show_message(ui);
        false
    }

    fn analysis_controls(&mut self, ui: &mut Ui) {
        let transcription_available = self.speech_recognition
            && executable_exists("ffmpeg")
            && executable_exists(&self.whisper_path)
            && PathBuf::from(&self.model_path).is_file();
        if self.speech_recognition {
            if !executable_exists("ffmpeg") {
                theme::state_message(
                    ui,
                    theme::State::Attention,
                    "FFmpeg が見つかりません。README の導入手順を確認し、導入後にアプリを再起動してください。",
                );
            }
            if !executable_exists(&self.whisper_path) || !PathBuf::from(&self.model_path).is_file()
            {
                theme::state_message(
                    ui,
                    theme::State::Attention,
                    "Whisper または音声認識モデルが見つかりません。「実行ファイルとモデルの設定」で選択してください。",
                );
            }
        }
        if self.analysis_task.is_some() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(16.0).color(theme::ACCENT));
                ui.label(theme::body("音声から情報を抽出中…"));
            });
            ui.label(theme::detail_muted(
                "Ollama 実行中の中断は応答を待って反映されます。",
            ));
            if theme::button(ui, "情報抽出を中断", theme::ButtonKind::Normal).clicked()
                && let Some(cancel) = &self.analysis_cancel
            {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        let start_clicked = ui
            .add_enabled_ui(
                self.analysis_task.is_none()
                    && self.material_id.is_some()
                    && transcription_available,
                |ui| theme::button(ui, "音声から情報を起こす", theme::ButtonKind::Primary),
            )
            .inner
            .clicked();
        if start_clicked
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
                    ffmpeg: PathBuf::from("ffmpeg"),
                    whisper: PathBuf::from(&self.whisper_path),
                    model: PathBuf::from(&self.model_path),
                    language: self.speech_language,
                };
                let ollama_model = self.ollama_model.trim().to_owned();
                let fingerprint = material.fingerprint.clone();
                let configuration = if config.language == SpeechLanguage::Auto {
                    format!("{} / {}", config.model.display(), ollama_model)
                } else {
                    format!(
                        "{} / {} / {}",
                        config.model.display(),
                        ollama_model,
                        config.language.code()
                    )
                };
                let previous = match self.store.analysis(id) {
                    Ok(record) => record,
                    Err(error) => {
                        self.message = Some((theme::State::Error, error.to_string()));
                        self.show_message(ui);
                        return;
                    }
                };
                let cached = previous.as_ref().and_then(|record| {
                    if record.status == AnalysisStatus::Completed
                        && record.fingerprint == fingerprint
                        && record.configuration == configuration
                    {
                        analysis_transcript(record).map(|transcript| (record.clone(), transcript))
                    } else {
                        None
                    }
                });
                if let Some((mut record, transcript)) = cached {
                    self.message = Some((
                        theme::State::Ready,
                        "保存済みの音声結果から情報を反映しています。".into(),
                    ));
                    let (sender, receiver) = mpsc::channel();
                    self.analysis_task = Some(receiver);
                    self.analysis_running_id = None;
                    let cancel = Arc::new(AtomicBool::new(false));
                    self.analysis_cancel = Some(Arc::clone(&cancel));
                    let ollama_model = ollama_model.clone();
                    std::thread::spawn(move || {
                        let mut suggestions = analysis_suggestions(&record);
                        if suggestions.is_none() && !ollama_model.is_empty() {
                            match ollama::suggest_metadata(&ollama_model, &transcript) {
                                Ok(metadata) => {
                                    record.error = None;
                                    record.candidates.extend(metadata_candidates(&metadata));
                                    suggestions = Some(metadata);
                                }
                                Err(error) => record.error = Some(error.to_string()),
                            }
                        }
                        let _ = sender.send((id, record, transcript, suggestions));
                    });
                    return;
                }
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
                        self.message =
                            Some((theme::State::Ready, "音声から情報を抽出しています。".into()));
                        let (sender, receiver) = mpsc::channel();
                        self.analysis_task = Some(receiver);
                        self.analysis_running_id = Some(id);
                        let cancel = Arc::new(AtomicBool::new(false));
                        self.analysis_cancel = Some(Arc::clone(&cancel));
                        std::thread::spawn(move || {
                            let mut suggestions = None;
                            let mut transcript = String::new();
                            let retain_previous_metadata = record.fingerprint == fingerprint;
                            let previous_metadata = record
                                .candidates
                                .iter()
                                .filter(|candidate| {
                                    candidate
                                        .evidence
                                        .as_deref()
                                        .is_some_and(|evidence| evidence.starts_with("metadata:"))
                                })
                                .cloned()
                                .collect::<Vec<_>>();
                            match local_stt::transcribe(&config, &path, &fingerprint, &cancel) {
                                Ok(candidates) => {
                                    let mut cached_metadata = if retain_previous_metadata {
                                        previous_metadata
                                    } else {
                                        Vec::new()
                                    };
                                    transcript = candidates
                                        .iter()
                                        .map(|candidate| candidate.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    if !ollama_model.is_empty()
                                        && !candidates.is_empty()
                                        && !cancel.load(Ordering::Relaxed)
                                    {
                                        match ollama::suggest_metadata(&ollama_model, &transcript) {
                                            Ok(metadata) => {
                                                cached_metadata = metadata_candidates(&metadata);
                                                suggestions = Some(metadata);
                                            }
                                            Err(error) => {
                                                record.error = Some(error.to_string());
                                            }
                                        }
                                    }
                                    if cancel.load(Ordering::Relaxed) {
                                        record.status = AnalysisStatus::Interrupted;
                                    } else {
                                        record.fingerprint = fingerprint;
                                        record.configuration = configuration;
                                        record.candidates = cached_metadata;
                                        record.candidates.extend(candidates);
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
                            let _ = sender.send((id, record, transcript, suggestions));
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
        ui.label(theme::detail_muted(if self.ollama_model.is_empty() {
            "実行すると文字起こしが検索語句に入ります。設定で Ollama を選ぶと、タグと役割も補います。"
        } else {
            "実行すると検索語句・タグ・役割に情報を反映します。既存の値に重複しないよう追加します。"
        }));
        self.show_message(ui);
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
            self.remember_draft(self.material_id);
            self.material_id = Some(id);
            self.load_selected();
            self.active_tab = EditorTab::Metadata;
        }
        self.show_message(ui);
        false
    }

    fn metadata_controls(&mut self, ui: &mut Ui) -> bool {
        ui.label(theme::body(
            "未保存の入力は 02・03 に移動しても保持されます。01 に戻って保存してください。",
        ));
        let selected = self.material_id;
        let material_selector_width = (ui.available_width() - theme::SPACE_SM).max(120.0);
        egui::ComboBox::from_id_salt("material")
            .width(material_selector_width)
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
            self.remember_draft(selected);
            self.load_selected();
        }
        ui.separator();
        let metadata_input_width = (ui.available_width() - 150.0).max(120.0);
        egui::Grid::new("metadata").num_columns(2).show(ui, |ui| {
            for (label, value) in [
                ("検索語句", &mut self.keywords),
                ("タグ", &mut self.tags),
                ("制作上の役割", &mut self.roles),
                ("メモ", &mut self.note),
            ] {
                ui.label(theme::detail(label));
                theme::text_input(ui, value, "", metadata_input_width);
                ui.end_row();
            }
        });
        ui.label(theme::detail_muted(
            "複数の検索語句・タグ・役割はカンマ区切りです。",
        ));
        ui.separator();
        ui.label(theme::detail_muted(
            "設定タブで方式を選び、文字起こしを実行できます。",
        ));
        self.analysis_controls(ui);
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

    fn remember_draft(&mut self, id: Option<i64>) {
        if let Some(id) = id {
            self.drafts.insert(
                id,
                (
                    self.keywords.clone(),
                    self.tags.clone(),
                    self.roles.clone(),
                    self.note.clone(),
                ),
            );
        }
    }

    fn apply_suggestions(
        &mut self,
        id: i64,
        transcript: &str,
        suggestions: Option<&ollama::MetadataSuggestions>,
    ) {
        let mut values = if self.material_id == Some(id) {
            (
                self.keywords.clone(),
                self.tags.clone(),
                self.roles.clone(),
                self.note.clone(),
            )
        } else if let Some(draft) = self.drafts.get(&id) {
            draft.clone()
        } else {
            match self.store.search(&SearchFilter::default()) {
                Ok(materials) => materials
                    .into_iter()
                    .find(|material| material.id == id)
                    .map(|material| {
                        (
                            material.metadata.keywords.join(", "),
                            material.metadata.tags.join(", "),
                            material.metadata.roles.join(", "),
                            material.metadata.note,
                        )
                    })
                    .unwrap_or_default(),
                Err(error) => {
                    self.message = Some((theme::State::Error, error.to_string()));
                    return;
                }
            }
        };

        append_metadata_values(&mut values.0, &[transcript.to_owned()]);
        if let Some(suggestions) = suggestions {
            append_metadata_values(&mut values.0, &suggestions.keywords);
            append_metadata_values(&mut values.1, &suggestions.tags);
            append_metadata_values(&mut values.2, &suggestions.roles);
        }
        self.drafts.insert(id, values.clone());
        if self.material_id == Some(id) {
            self.keywords = values.0;
            self.tags = values.1;
            self.roles = values.2;
            self.note = values.3;
        }
    }

    fn load_selected(&mut self) {
        let Some(id) = self.material_id else {
            return;
        };
        if let Some((keywords, tags, roles, note)) = self.drafts.get(&id) {
            self.keywords.clone_from(keywords);
            self.tags.clone_from(tags);
            self.roles.clone_from(roles);
            self.note.clone_from(note);
            return;
        }
        let metadata = match self.store.search(&SearchFilter::default()) {
            Ok(materials) => materials
                .into_iter()
                .find(|material| material.id == id)
                .map(|material| material.metadata)
                .unwrap_or_default(),
            Err(error) => {
                self.message = Some((theme::State::Error, error.to_string()));
                return;
            }
        };
        self.keywords = metadata.keywords.join(", ");
        self.tags = metadata.tags.join(", ");
        self.roles = metadata.roles.join(", ");
        self.note = metadata.note;
        self.message = None;
    }

    fn has_unsaved_metadata(&self) -> Result<bool, CatalogError> {
        let stored_materials = self.store.search(&SearchFilter::default())?;
        let differs = |id: i64, keywords: &str, tags: &str, roles: &str, note: &str| {
            let current = metadata_from_fields(keywords, tags, roles, note);
            stored_materials
                .iter()
                .find(|material| material.id == id)
                .map(|material| &material.metadata)
                != Some(&current)
        };

        for (id, (keywords, tags, roles, note)) in &self.drafts {
            if differs(*id, keywords, tags, roles, note) {
                return Ok(true);
            }
        }
        if let Some(id) = self.material_id
            && differs(id, &self.keywords, &self.tags, &self.roles, &self.note)
        {
            return Ok(true);
        }
        Ok(false)
    }

    fn save_all_drafts(&mut self) -> Result<(), CatalogError> {
        let mut pending = self.drafts.clone();
        if let Some(id) = self.material_id {
            pending.insert(
                id,
                (
                    self.keywords.clone(),
                    self.tags.clone(),
                    self.roles.clone(),
                    self.note.clone(),
                ),
            );
        }
        for (id, (keywords, tags, roles, note)) in pending {
            let metadata = metadata_from_fields(&keywords, &tags, &roles, &note);
            self.store.save_metadata(id, &metadata)?;
        }
        self.drafts.clear();
        self.refresh()?;
        self.load_selected();
        Ok(())
    }

    fn discard_changes(&mut self) {
        self.drafts.clear();
        self.load_selected();
    }

    fn show_message(&self, ui: &mut Ui) {
        if let Some((state, message)) = &self.message
            && *state != theme::State::Ready
        {
            theme::state_message(ui, *state, message);
        }
    }

    fn save(&mut self) -> Result<(), CatalogError> {
        let material_id = self.material_id.ok_or(CatalogError::NoMaterialSelected)?;
        let metadata = metadata_from_fields(&self.keywords, &self.tags, &self.roles, &self.note);
        self.store.save_metadata(material_id, &metadata)?;
        self.drafts.remove(&material_id);
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
            record.error = Some("アプリ終了により情報抽出を中断しました".into());
            let _ = self.store.save_analysis(id, &record);
        }
    }
}

fn append_metadata_values(target: &mut String, additions: &[String]) {
    let mut values = Vec::new();
    for value in split(target) {
        let parts: Vec<_> = value.split("」「").collect();
        if parts.len() > 1
            && parts.iter().all(|part| {
                let part = normalize_metadata_value(part);
                !part.is_empty()
                    && additions
                        .iter()
                        .any(|addition| normalize_metadata_value(addition) == part)
            })
        {
            values.extend(
                parts
                    .into_iter()
                    .map(|part| part.trim().trim_matches(['「', '」']).trim().to_owned()),
            );
        } else {
            values.push(value);
        }
    }
    for addition in additions.iter().map(|value| value.trim()) {
        if !addition.is_empty()
            && !values
                .iter()
                .any(|value| normalize_metadata_value(value) == normalize_metadata_value(addition))
        {
            values.push(addition.to_owned());
        }
    }
    *target = values.join(", ");
}

fn analysis_transcript(record: &AnalysisRecord) -> Option<String> {
    let transcript = record
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.evidence.as_deref() != Some("文字起こし候補の要約")
                && !candidate
                    .evidence
                    .as_deref()
                    .is_some_and(|evidence| evidence.starts_with("metadata:"))
        })
        .map(|candidate| candidate.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    (!transcript.is_empty()).then_some(transcript)
}

fn metadata_candidates(suggestions: &ollama::MetadataSuggestions) -> Vec<AnalysisCandidate> {
    let mut candidates = vec![AnalysisCandidate {
        text: String::new(),
        original_text: None,
        language: None,
        start_ms: 0,
        end_ms: 0,
        model: "Ollama".into(),
        model_version: String::new(),
        confidence: None,
        evidence: Some("metadata:complete".into()),
    }];
    candidates.extend(
        [
            ("metadata:keyword", &suggestions.keywords),
            ("metadata:tag", &suggestions.tags),
            ("metadata:role", &suggestions.roles),
        ]
        .into_iter()
        .flat_map(|(evidence, values)| {
            values.iter().map(move |value| AnalysisCandidate {
                text: value.clone(),
                original_text: None,
                language: None,
                start_ms: 0,
                end_ms: 0,
                model: "Ollama".into(),
                model_version: String::new(),
                confidence: None,
                evidence: Some(evidence.into()),
            })
        }),
    );
    candidates
}

fn analysis_suggestions(record: &AnalysisRecord) -> Option<ollama::MetadataSuggestions> {
    let values = |evidence: &str| {
        record
            .candidates
            .iter()
            .filter(|candidate| candidate.evidence.as_deref() == Some(evidence))
            .map(|candidate| candidate.text.clone())
            .collect::<Vec<_>>()
    };
    let suggestions = ollama::MetadataSuggestions {
        keywords: values("metadata:keyword"),
        tags: values("metadata:tag"),
        roles: values("metadata:role"),
    };
    record
        .candidates
        .iter()
        .any(|candidate| candidate.evidence.as_deref() == Some("metadata:complete"))
        .then(|| suggestions.normalized())
}

fn normalize_metadata_value(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn metadata_from_fields(keywords: &str, tags: &str, roles: &str, note: &str) -> Metadata {
    Metadata {
        keywords: split(keywords),
        tags: split(tags),
        roles: split(roles),
        note: note.to_owned(),
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

pub(super) fn executable_exists(value: &str) -> bool {
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
    fn editor_settings_round_trip_preserves_selected_models_and_paths() {
        let settings = EditorSettings {
            whisper_path: "tools/whisper-cli.exe".into(),
            model_path: "models/ggml-small.bin".into(),
            ollama_model: "gemma4:e2b".into(),
            speech_recognition: true,
            speech_language: SpeechLanguage::Japanese,
        };

        let encoded = serde_json::to_string(&settings).unwrap();
        let restored: EditorSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(restored, settings);
    }

    #[test]
    fn missing_ffmpeg_does_not_prevent_catalog_search() {
        const CHILD: &str = "CUTUP_TEST_WITHOUT_FFMPEG";
        if std::env::var_os(CHILD).is_some() {
            assert!(!executable_exists("ffmpeg"));
            editor_saved_material_metadata_survives_reopening_and_produces_candidates();
            return;
        }

        let directory = tempfile::tempdir().unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("ui::catalog_editor::tests::missing_ffmpeg_does_not_prevent_catalog_search")
            .env(CHILD, "1")
            .env("PATH", directory.path())
            .current_dir(directory.path())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
    #[test]
    fn switching_materials_keeps_unsaved_input_and_save_makes_it_searchable() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("a.wav"), b"a").unwrap();
        std::fs::write(materials.join("b.wav"), b"b").unwrap();
        let mut store =
            SqliteCatalog::create(&temp.path().join("catalog.sqlite"), temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::with_settings(store, EditorSettings::default()).unwrap();
        let first = editor.material_id;
        editor.keywords = "既存の入力".into();
        append_metadata_values(&mut editor.keywords, &["文字起こし".into()]);
        editor.roles = "導入".into();
        editor.remember_draft(first);
        editor.material_id = Some(editor.materials[1].id);
        editor.load_selected();
        editor.remember_draft(editor.material_id);
        editor.material_id = first;
        editor.load_selected();
        assert_eq!(editor.keywords, "既存の入力, 文字起こし");
        editor.save().unwrap();
        assert_eq!(
            create_plan(
                &editor.store.candidate_catalog().unwrap(),
                "導入: 文字起こし"
            )
            .slots[0]
                .candidates
                .len(),
            1
        );
    }

    #[test]
    fn failed_analysis_preserves_unsaved_metadata() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("a.wav"), b"a").unwrap();
        let mut store =
            SqliteCatalog::create(&temp.path().join("catalog.sqlite"), temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::with_settings(store, EditorSettings::default()).unwrap();
        editor.keywords = "編集中".into();
        editor.note = "残すメモ".into();
        let (sender, receiver) = mpsc::channel();
        editor.analysis_task = Some(receiver);
        sender
            .send((
                editor.material_id.unwrap(),
                AnalysisRecord {
                    fingerprint: "test".into(),
                    configuration: "test".into(),
                    status: AnalysisStatus::Failed,
                    candidates: vec![],
                    error: Some("失敗".into()),
                },
                String::new(),
                None,
            ))
            .unwrap();
        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.controls(ui);
            });
        });
        assert_eq!(editor.keywords, "編集中");
        assert_eq!(editor.note, "残すメモ");
        assert!(editor.message().is_some());
    }

    #[test]
    fn metadata_suggestions_are_not_loaded_again_after_discard() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("nani.wav"), b"audio").unwrap();
        let path = temp.path().join("catalog.sqlite");
        let mut store = SqliteCatalog::create(&path, temp.path()).unwrap();
        store.rescan().unwrap();
        let material = store.materials().unwrap().remove(0);
        let mut record = AnalysisRecord {
            fingerprint: material.fingerprint,
            configuration: "Whisper / Ollama".into(),
            status: AnalysisStatus::Completed,
            candidates: vec![AnalysisCandidate {
                text: "何?".into(),
                original_text: None,
                language: Some("ja".into()),
                start_ms: 0,
                end_ms: 900,
                model: "whisper".into(),
                model_version: "test".into(),
                confidence: None,
                evidence: Some("音声から文字起こし候補".into()),
            }],
            error: None,
        };
        record
            .candidates
            .extend(metadata_candidates(&ollama::MetadataSuggestions {
                keywords: vec!["何".into()],
                tags: vec!["質問」「確認".into()],
                roles: vec!["反応".into()],
            }));
        store.save_analysis(material.id, &record).unwrap();
        drop(store);

        let mut editor = CatalogEditor::with_settings(
            SqliteCatalog::open(&path).unwrap(),
            EditorSettings::default(),
        )
        .unwrap();
        assert!(editor.keywords.is_empty());
        assert!(editor.tags.is_empty());
        assert!(editor.roles.is_empty());
        assert!(!editor.has_unsaved_metadata().unwrap());

        let cached_record = editor.store.analysis(material.id).unwrap().unwrap();
        let transcript = analysis_transcript(&cached_record).unwrap();
        let suggestions = analysis_suggestions(&cached_record).unwrap();
        editor.apply_suggestions(material.id, &transcript, Some(&suggestions));
        assert_eq!(editor.keywords, "何?");
        assert_eq!(editor.tags, "質問, 確認");
        assert_eq!(editor.roles, "反応");
        assert!(editor.has_unsaved_metadata().unwrap());
        editor.discard_changes();
        assert!(editor.keywords.is_empty());
        assert!(editor.tags.is_empty());
        assert!(editor.roles.is_empty());
        assert!(!editor.has_unsaved_metadata().unwrap());
        drop(editor);

        let mut editor = CatalogEditor::with_settings(
            SqliteCatalog::open(&path).unwrap(),
            EditorSettings::default(),
        )
        .unwrap();
        assert!(editor.keywords.is_empty());
        assert!(editor.tags.is_empty());
        assert!(editor.roles.is_empty());
        let cached_record = editor.store.analysis(material.id).unwrap().unwrap();
        let transcript = analysis_transcript(&cached_record).unwrap();
        let suggestions = analysis_suggestions(&cached_record).unwrap();
        editor.apply_suggestions(material.id, &transcript, Some(&suggestions));
        editor.save().unwrap();
        assert!(!editor.has_unsaved_metadata().unwrap());

        let reopened = CatalogEditor::with_settings(
            SqliteCatalog::open(&path).unwrap(),
            EditorSettings::default(),
        )
        .unwrap();
        assert_eq!(reopened.keywords, "何?");
        assert_eq!(reopened.tags, "質問, 確認");
        assert_eq!(reopened.roles, "反応");
    }

    #[test]
    fn audio_suggestions_fill_metadata_fields_and_preserve_existing_values() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("a.wav"), b"a").unwrap();
        let mut store =
            SqliteCatalog::create(&temp.path().join("catalog.sqlite"), temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::with_settings(store, EditorSettings::default()).unwrap();
        let id = editor.material_id.unwrap();
        editor.keywords = "手入力語".into();
        editor.tags = "手入力タグ, 質問」「確認, 手動」「入力".into();
        editor.roles = "導入".into();
        editor.note = "利用者のメモ".into();
        editor.apply_suggestions(
            id,
            "なに?",
            Some(&ollama::MetadataSuggestions {
                keywords: vec!["なに".into(), "補助キーワード".into()],
                tags: vec![
                    "反応".into(),
                    "手入力タグ".into(),
                    "質問".into(),
                    "確認".into(),
                ],
                roles: vec!["リアクション".into()],
            }),
        );
        assert_eq!(editor.keywords, "手入力語, なに?, 補助キーワード");
        assert_eq!(editor.tags, "手入力タグ, 質問, 確認, 手動」「入力, 反応");
        assert_eq!(editor.roles, "導入, リアクション");
        assert_eq!(editor.note, "利用者のメモ");
    }

    #[test]
    fn unavailable_ollama_preserves_selected_model_and_clears_available_models() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("sample.wav"), b"local media fixture").unwrap();
        let path = temp.path().join("catalog.sqlite");
        let mut store = SqliteCatalog::create(&path, temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::with_settings(store, EditorSettings::default()).unwrap();
        editor.active_tab = EditorTab::Analysis;
        editor.ollama_model = "gemma4:e2b".into();
        editor.ollama_models = vec!["gemma4:e2b".into()];
        let (sender, receiver) = mpsc::channel();
        editor.ollama_models_task = Some(receiver);
        sender.send(Err(ollama::OllamaError::Unavailable)).unwrap();

        let context = egui::Context::default();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                editor.controls(ui);
            });
        });

        assert_eq!(editor.ollama_model, "gemma4:e2b");
        assert!(editor.ollama_models.is_empty());
        assert_eq!(editor.ollama_available, Some(false));
        assert_eq!(editor.settings().ollama_model, "gemma4:e2b");
    }

    #[test]
    fn editor_saved_material_metadata_survives_reopening_and_produces_candidates() {
        let temp = tempfile::tempdir().unwrap();
        let materials = temp.path().join("materials");
        std::fs::create_dir(&materials).unwrap();
        std::fs::write(materials.join("sample.wav"), b"local media fixture").unwrap();
        let path = temp.path().join("catalog.sqlite");
        let mut store = SqliteCatalog::create(&path, temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::with_settings(store, EditorSettings::default()).unwrap();
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
        let reopened = CatalogEditor::with_settings(
            SqliteCatalog::open(&path).unwrap(),
            EditorSettings::default(),
        )
        .unwrap();
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
