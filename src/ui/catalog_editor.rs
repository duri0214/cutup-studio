use crate::{
    domain::valueobject::catalog::*,
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
};
use eframe::egui::{self, Ui};

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
    message: String,
}

impl CatalogEditor {
    pub fn new(store: SqliteCatalog) -> Result<Self, CatalogError> {
        let materials = store.materials()?;
        let stored_materials = store.search(&SearchFilter {
            include_unavailable: true,
            ..Default::default()
        })?;
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
            message: String::new(),
        };
        editor.load_selected();
        Ok(editor)
    }

    pub fn counts(&self) -> (usize, usize) {
        (
            self.materials.len(),
            self.materials
                .iter()
                .filter(|material| !material.available)
                .count(),
        )
    }

    pub fn show(&mut self, context: &egui::Context, enabled: bool) -> bool {
        let mut open = self.open;
        let mut changed = false;
        egui::Window::new("素材の情報を登録")
            .open(&mut open)
            .default_width(740.0)
            .default_height(520.0)
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
        ui.strong("仮組みでは、素材に入っている言葉・タグ・役割を手で登録します。");
        ui.label("将来はローカルLLMがここを自動で埋めます。");
        ui.label("開始・終了時刻は入力しません。素材ファイル1本が、そのまま候補の単位です。");
        if self.materials.is_empty() {
            ui.separator();
            ui.strong("まだ登録できる素材がありません。");
            ui.label("1. 選んだ素材フォルダに動画または音声ファイルを入れます。");
            ui.label("2. この画面を閉じ、01 / MATERIAL CATALOG の更新アイコンを押します。");
            return false;
        }
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
            self.load_selected();
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("素材検索");
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
                    Err(error) => self.message = error.to_string(),
                }
            }
        });
        ui.label(
            "利用不能・要確認の素材も表示します。GUIDEの候補には利用可能な素材だけを使います。",
        );
        let mut selection = None;
        egui::ScrollArea::vertical()
            .id_salt("stored_materials")
            .max_height(130.0)
            .show(ui, |ui| {
                for material in &self.stored_materials {
                    if ui
                        .selectable_label(
                            self.material_id == Some(material.id),
                            format!(
                                "{} / {} / {}",
                                material.source_path,
                                material.availability.label(),
                                material.metadata.keywords.join(", ")
                            ),
                        )
                        .clicked()
                    {
                        selection = Some(material.id);
                    }
                }
            });
        if let Some(id) = selection {
            self.material_id = Some(id);
            self.load_selected();
        }
        ui.separator();
        ui.label("選択中の素材の情報");
        egui::Grid::new("metadata").num_columns(2).show(ui, |ui| {
            for (label, value) in [
                ("検索語句", &mut self.keywords),
                ("タグ", &mut self.tags),
                ("制作上の役割", &mut self.roles),
                ("メモ", &mut self.note),
            ] {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(value).desired_width(510.0));
                ui.end_row();
            }
        });
        ui.label("複数の検索語句・タグ・役割はカンマ区切りです。");
        let available = self
            .materials
            .iter()
            .any(|material| Some(material.id) == self.material_id && material.available);
        let mut changed = false;
        if ui
            .add_enabled(available, egui::Button::new("素材の情報を保存"))
            .clicked()
        {
            match self.save() {
                Ok(()) => {
                    self.message = "素材の情報を保存しました。GUIDEから候補を生成できます。".into();
                    changed = true;
                }
                Err(error) => self.message = error.to_string(),
            }
        }
        ui.label(&self.message);
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
        self.message.clear();
    }

    fn save(&mut self) -> Result<(), CatalogError> {
        let material_id = self.material_id.ok_or(CatalogError::Unavailable)?;
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
            include_unavailable: true,
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
        std::fs::write(temp.path().join("sample.wav"), b"local media fixture").unwrap();
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
            editor.show(ctx, true);
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
