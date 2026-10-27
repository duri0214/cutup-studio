use crate::{
    domain::valueobject::catalog::*,
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
};
use eframe::egui::{self, Ui};

pub(super) struct CatalogEditor {
    pub store: SqliteCatalog,
    pub open: bool,
    materials: Vec<Material>,
    segments: Vec<StoredSegment>,
    material_id: Option<i64>,
    segment_id: Option<i64>,
    start_ms: i64,
    end_ms: i64,
    transcript: String,
    keywords: String,
    speakers: String,
    visual_tags: String,
    audio_tags: String,
    tags: String,
    roles: String,
    note: String,
    original: Metadata,
    manual: ManualMetadata,
    query: String,
    tag_filter: String,
    role_filter: String,
    message: String,
}

impl CatalogEditor {
    pub fn new(store: SqliteCatalog) -> Result<Self, CatalogError> {
        let materials = store.materials()?;
        let segments = store.search(&SearchFilter {
            include_unavailable: true,
            ..Default::default()
        })?;
        Ok(Self {
            store,
            open: true,
            material_id: materials.first().map(|m| m.id),
            materials,
            segments,
            segment_id: None,
            start_ms: 0,
            end_ms: 1000,
            transcript: String::new(),
            keywords: String::new(),
            speakers: String::new(),
            visual_tags: String::new(),
            audio_tags: String::new(),
            tags: String::new(),
            roles: "導入".into(),
            note: String::new(),
            original: Metadata::default(),
            manual: ManualMetadata::default(),
            query: String::new(),
            tag_filter: String::new(),
            role_filter: String::new(),
            message: String::new(),
        })
    }

    pub fn counts(&self) -> (usize, usize) {
        (
            self.materials.len(),
            self.materials.iter().filter(|m| !m.available).count(),
        )
    }

    pub fn show(&mut self, context: &egui::Context, enabled: bool) -> bool {
        let mut open = self.open;
        let mut changed = false;
        egui::Window::new("素材と区間の編集")
            .open(&mut open)
            .default_width(740.0)
            .default_height(620.0)
            .vscroll(true)
            .show(context, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    changed = self.controls(ui);
                });
            });
        self.open = open;
        changed
    }

    fn controls(&mut self, ui: &mut Ui) -> bool {
        ui.label("素材を選び、開始・終了時刻と検索に使う語句を入力します。時間はミリ秒です。");
        ui.label("動画再生・自動解析は未対応です。外部プレイヤーで区間を確認してください。");
        if self.materials.is_empty() {
            ui.separator();
            ui.strong("まだ編集できる素材がありません。");
            ui.label("SQLite カタログだけが作成されており、素材ファイルは登録されていません。");
            ui.label("1. 選んだ素材フォルダに動画または音声ファイルを入れます。");
            ui.label("2. この画面を閉じ、01 / MATERIAL CATALOG の RESCAN を押します。");
            ui.label("3. 素材が表示されたら、この画面で区間とタグを登録します。");
            return false;
        }
        let selected = self.material_id;
        egui::ComboBox::from_id_salt("material")
            .width(640.0)
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
                        format!(
                            "{}{}",
                            material.source_path,
                            if material.available {
                                ""
                            } else {
                                "（利用不能）"
                            }
                        ),
                    );
                }
            });
        if selected != self.material_id {
            self.new_segment();
        }
        if ui.button("この素材に新しい区間を追加").clicked() {
            self.new_segment();
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("区間検索");
            ui.text_edit_singleline(&mut self.query);
        });
        ui.horizontal(|ui| {
            ui.label("タグ");
            ui.add(egui::TextEdit::singleline(&mut self.tag_filter).desired_width(130.0));
            ui.label("役割");
            ui.add(egui::TextEdit::singleline(&mut self.role_filter).desired_width(130.0));
            if ui.button("検索").clicked() {
                match self.refresh() {
                    Ok(()) => self.message.clear(),
                    Err(e) => self.message = e.to_string(),
                }
            }
        });
        ui.label(
            "利用不能・要確認の区間も表示します。GUIDE の候補には利用可能な区間だけを使います。",
        );
        let mut selection = None;
        egui::ScrollArea::vertical()
            .id_salt("stored_segments")
            .max_height(150.0)
            .show(ui, |ui| {
                for segment in &self.segments {
                    if ui
                        .selectable_label(
                            self.segment_id == Some(segment.id),
                            format!(
                                "#{} {} / {}–{} ms / {} / {}",
                                segment.id,
                                segment.source_path,
                                segment.range.start_ms(),
                                segment.range.end_ms(),
                                segment.availability.label(),
                                segment.metadata.transcript
                            ),
                        )
                        .clicked()
                    {
                        selection = Some(segment.clone());
                    }
                }
            });
        if let Some(segment) = selection {
            self.select(segment);
        }
        ui.separator();
        ui.label(
            self.segment_id
                .map_or_else(|| "新規区間".into(), |id| format!("区間 #{id} を編集")),
        );
        ui.horizontal(|ui| {
            ui.label("開始 ms");
            ui.add(egui::DragValue::new(&mut self.start_ms).range(0..=i64::MAX));
            ui.label("終了 ms");
            ui.add(egui::DragValue::new(&mut self.end_ms).range(0..=i64::MAX));
        });
        egui::Grid::new("metadata").num_columns(2).show(ui, |ui| {
            for (label, value) in [
                ("文字起こし", &mut self.transcript),
                ("話者候補", &mut self.speakers),
                ("検索語句", &mut self.keywords),
                ("映像タグ", &mut self.visual_tags),
                ("音響タグ", &mut self.audio_tags),
                ("手動タグ", &mut self.tags),
                ("制作上の役割", &mut self.roles),
                ("メモ", &mut self.note),
            ] {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(value).desired_width(510.0));
                ui.end_row();
            }
        });
        ui.label("複数の語句・話者・タグ・役割はカンマ区切り。既存区間は変更した項目だけを手動修正として保存します。");
        let available = self
            .materials
            .iter()
            .any(|m| Some(m.id) == self.material_id && m.available);
        let mut changed = false;
        if ui
            .add_enabled(
                available,
                egui::Button::new("区間を保存（現在の素材で確認済みにする）"),
            )
            .clicked()
        {
            match self.save() {
                Ok(id) => {
                    self.segment_id = Some(id);
                    self.message =
                        format!("区間 #{id} を保存しました。GUIDE から候補を生成できます。");
                    changed = true;
                }
                Err(error) => self.message = error.to_string(),
            }
        }
        if let Some(id) = self.segment_id {
            ui.collapsing("AI 解析の根拠・履歴", |ui| {
                match self.store.analysis_history(id) {
                    Ok(history) if history.is_empty() => {
                        ui.label("AI 解析結果はありません。");
                    }
                    Ok(history) => {
                        for record in history {
                            ui.label(format!(
                                "{} / {} / 確度 {:.2} / {}",
                                record.model, record.version, record.confidence, record.created_at
                            ));
                            ui.label(format!(
                                "解析区間: {}–{} ms / 文字起こし: {}",
                                record.range.start_ms(),
                                record.range.end_ms(),
                                record.metadata.transcript
                            ));
                            ui.label(format!("入力 SHA-256: {}", record.input_fingerprint));
                            ui.label(format!("根拠: {}", record.evidence));
                        }
                    }
                    Err(error) => {
                        ui.label(error.to_string());
                    }
                }
            });
        }
        ui.label(&self.message);
        changed
    }

    fn new_segment(&mut self) {
        self.segment_id = None;
        self.start_ms = 0;
        self.end_ms = 1000;
        self.set_fields(&Metadata {
            roles: vec!["導入".into()],
            ..Default::default()
        });
        self.original = Metadata::default();
        self.manual = ManualMetadata::default();
        self.message.clear();
    }

    fn select(&mut self, segment: StoredSegment) {
        self.material_id = Some(segment.material_id);
        self.segment_id = Some(segment.id);
        self.start_ms = segment.range.start_ms();
        self.end_ms = segment.range.end_ms();
        self.set_fields(&segment.metadata);
        self.original = segment.metadata;
        self.manual = segment.manual;
        self.message.clear();
    }

    fn set_fields(&mut self, m: &Metadata) {
        self.transcript.clone_from(&m.transcript);
        self.note.clone_from(&m.note);
        self.keywords = m.keywords.join(", ");
        self.speakers = m.speakers.join(", ");
        self.visual_tags = m.visual_tags.join(", ");
        self.audio_tags = m.audio_tags.join(", ");
        self.tags = m.tags.join(", ");
        self.roles = m.roles.join(", ");
    }

    fn save(&mut self) -> Result<i64, CatalogError> {
        let material = self.material_id.ok_or(CatalogError::Unavailable)?;
        let range = TimeRange::new(self.start_ms, self.end_ms)?;
        let mut manual = self.manual.clone();
        for (value, original, target) in [
            (
                split(&self.keywords),
                &self.original.keywords,
                &mut manual.keywords,
            ),
            (
                split(&self.speakers),
                &self.original.speakers,
                &mut manual.speakers,
            ),
            (
                split(&self.visual_tags),
                &self.original.visual_tags,
                &mut manual.visual_tags,
            ),
            (
                split(&self.audio_tags),
                &self.original.audio_tags,
                &mut manual.audio_tags,
            ),
            (split(&self.tags), &self.original.tags, &mut manual.tags),
            (split(&self.roles), &self.original.roles, &mut manual.roles),
        ] {
            if value != *original {
                *target = Some(value);
            }
        }
        if self.transcript != self.original.transcript {
            manual.transcript = Some(self.transcript.clone());
        }
        if self.note != self.original.note {
            manual.note = Some(self.note.clone());
        }
        let id = self
            .store
            .save_manual(self.segment_id, material, range, &manual)?;
        self.refresh()?;
        // Search filters may hide the saved row; reload its effective values independently.
        if let Some(segment) = self
            .store
            .search(&SearchFilter {
                include_unavailable: true,
                ..Default::default()
            })?
            .into_iter()
            .find(|s| s.id == id)
        {
            self.select(segment);
        }
        Ok(id)
    }

    fn refresh(&mut self) -> Result<(), CatalogError> {
        self.segments = self.store.search(&SearchFilter {
            text: &self.query,
            tag: nonempty(&self.tag_filter),
            role: nonempty(&self.role_filter),
            include_unavailable: true,
        })?;
        Ok(())
    }
}

fn split(text: &str) -> Vec<String> {
    text.split([',', '、'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
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
    fn editor_saved_fields_survive_reopening_and_produce_guide_candidates() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("sample.wav"), b"local media fixture").unwrap();
        let path = temp.path().join("catalog.sqlite");
        let mut store = SqliteCatalog::create(&path, temp.path()).unwrap();
        store.rescan().unwrap();
        let mut editor = CatalogEditor::new(store).unwrap();
        editor.start_ms = 100;
        editor.end_ms = 800;
        editor.transcript = "テッカマン登場".into();
        editor.tags = "画, 反応".into();
        editor.roles = "導入".into();
        let id = editor.save().unwrap();
        // Rendering the actual editor also catches widget/state errors without a native dialog.
        let context = egui::Context::default();
        let output = context.run(egui::RawInput::default(), |ctx| {
            editor.show(ctx, true);
        });
        assert!(!output.shapes.is_empty());
        drop(editor);
        let mut reopened = CatalogEditor::new(SqliteCatalog::open(&path).unwrap()).unwrap();
        let row = reopened.segments[0].clone();
        assert_eq!(row.id, id);
        assert_eq!(row.metadata.tags, vec!["画", "反応"]);
        reopened.select(row);
        reopened.transcript = "修正したテッカマン".into();
        assert_eq!(reopened.save().unwrap(), id);
        let catalog = reopened.store.candidate_catalog().unwrap();
        let plan = create_plan(&catalog, "導入: テッカマン");
        assert_eq!(plan.slots[0].candidates.len(), 1);
        assert_eq!(plan.slots[0].candidates[0].transcript, "修正したテッカマン");
        assert_eq!(plan.slots[0].candidates[0].tags, vec!["画", "反応"]);
    }
}
