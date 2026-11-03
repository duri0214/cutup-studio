use crate::{
    domain::valueobject::catalog::*,
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
    ui::theme,
};
use eframe::egui::{self, Ui};

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorTab {
    Search,
    Metadata,
}

pub(super) const PANEL_WIDTH: f32 = 640.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PanelEvent {
    None,
    Saved,
    ReturnToCatalog,
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
}

impl CatalogEditor {
    pub fn new(store: SqliteCatalog) -> Result<Self, CatalogError> {
        let materials = store.materials()?;
        let stored_materials = store.search(&SearchFilter::default())?;
        let material_id = materials.first().map(|material| material.id);
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
                ui.label(theme::caption_text("素材の情報を登録"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if theme::button(ui, "01へ戻る", theme::ButtonKind::Normal).clicked() {
                        self.open = false;
                        event = PanelEvent::ReturnToCatalog;
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
        ui.label(
            theme::body("仮組みでは、素材に入っている言葉・タグ・役割を手で登録します。").strong(),
        );
        ui.label(theme::body("将来はローカルLLMがここを自動で埋めます。"));
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
            Self::tab_button(
                ui,
                &mut self.active_tab,
                EditorTab::Metadata,
                "素材の情報を登録",
            );
        });
        ui.add_space(theme::SPACE_SM);
        match self.active_tab {
            EditorTab::Search => self.search_controls(ui),
            EditorTab::Metadata => self.metadata_controls(ui),
        }
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
            "保存して次へ"
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

fn split(text: &str) -> Vec<String> {
    text.split([',', '、'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
