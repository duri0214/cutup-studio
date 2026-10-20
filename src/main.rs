use cutup_studio::{
    Catalog, Plan, SCHEMA_VERSION, create_plan, manifest_from_plan, validate_catalog,
};
use eframe::egui::{self, Color32, RichText, Stroke, Vec2};
use std::{collections::BTreeMap, fs, path::PathBuf};

const MIKU: Color32 = Color32::from_rgb(57, 255, 204);
const MIKU_DIM: Color32 = Color32::from_rgb(21, 93, 80);
const INK: Color32 = Color32::from_rgb(9, 14, 22);
const PANEL: Color32 = Color32::from_rgb(17, 28, 40);
const SOFT: Color32 = Color32::from_rgb(164, 190, 204);

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "cutup-studio",
        options,
        Box::new(|cc| Ok(Box::new(StudioApp::new(cc)))),
    )
}

struct StudioApp {
    catalog: Option<Catalog>,
    catalog_path: Option<PathBuf>,
    guide_text: String,
    plan: Option<Plan>,
    selections: BTreeMap<String, String>,
    status: String,
}

impl StudioApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_visuals(&cc.egui_ctx);
        Self {
            catalog: None,
            catalog_path: None,
            guide_text: "導入: テッカマン\nつなぎ: 驚き 反応".into(),
            plan: None,
            selections: BTreeMap::new(),
            status: "カタログ JSON を読み込んでください。".into(),
        }
    }
    fn open_catalog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("catalog JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        match fs::read_to_string(&path)
            .ok()
            .and_then(|source| serde_json::from_str::<Catalog>(&source).ok())
        {
            Some(catalog) => match validate_catalog(&catalog) {
                Ok(()) => {
                    self.catalog_path = Some(path);
                    self.catalog = Some(catalog);
                    self.plan = None;
                    self.selections.clear();
                    self.status = "カタログを読み込みました。ガイドから候補を作れます。".into();
                }
                Err(error) => self.status = format!("カタログの検証に失敗: {error}"),
            },
            None => self.status = "JSON を読み込めませんでした。形式を確認してください。".into(),
        }
    }
    fn build_plan(&mut self) {
        let Some(catalog) = &self.catalog else {
            self.status = "先にカタログ JSON を読み込んでください。".into();
            return;
        };
        let plan = create_plan(catalog, &self.guide_text);
        if plan.slots.is_empty() {
            self.status = "ガイドを `役割: 語句 タグ` の形式で入力してください。".into();
            return;
        }
        self.selections = plan
            .slots
            .iter()
            .filter_map(|slot| {
                slot.candidates
                    .first()
                    .map(|candidate| (slot.slot.id.clone(), candidate.segment_id.clone()))
            })
            .collect();
        self.status = format!(
            "{} 個のスロットに候補を出しました。各行で素材を確定できます。",
            plan.slots.len()
        );
        self.plan = Some(plan);
    }
    fn save_manifest(&mut self) {
        let Some(plan) = &self.plan else {
            self.status = "先に候補を作ってください。".into();
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("composition-manifest.json")
            .save_file()
        else {
            return;
        };
        match manifest_from_plan(plan, &self.selections).and_then(|manifest| {
            serde_json::to_string_pretty(&manifest).map_err(|_| {
                cutup_studio::StudioError::MissingSelection("JSON の保存に失敗".into())
            })
        }) {
            Ok(json) => match fs::write(&path, json) {
                Ok(()) => self.status = format!("manifest を保存しました: {}", path.display()),
                Err(error) => self.status = format!("保存に失敗: {error}"),
            },
            Err(error) => self.status = format!("manifest を作れません: {error}"),
        }
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::NONE
                    .fill(INK)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("CUTUP//STUDIO")
                            .size(22.0)
                            .strong()
                            .color(MIKU),
                    );
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("LOCAL COMPOSITION CONSOLE")
                            .size(11.0)
                            .color(SOFT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("SCHEMA V{}", SCHEMA_VERSION))
                                .size(10.0)
                                .color(MIKU),
                        )
                    });
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(24, 10)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new(&self.status).size(12.0).color(SOFT))
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(INK).inner_margin(24))
            .show(ctx, |ui| {
                ui.columns(2, |columns| {
                    let left = &mut columns[0];
                    panel(left, "01 / CATALOG", |ui| {
                        let label = self
                            .catalog_path
                            .as_ref()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "未読込 — 素材は参照のみです".into());
                        ui.label(RichText::new(label).size(12.0).color(SOFT));
                        ui.add_space(10.0);
                        if action_button(ui, "CATALOG JSON を開く") {
                            self.open_catalog();
                        }
                        if let Some(catalog) = &self.catalog {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} segments  /  root: {}",
                                    catalog.segments.len(),
                                    catalog.material_root
                                ))
                                .color(MIKU)
                                .size(12.0),
                            );
                        }
                    });
                    let right = &mut columns[1];
                    panel(right, "02 / GUIDE", |ui| {
                        ui.label(
                            RichText::new("一行ごとに  役割: 探したい語句 タグ")
                                .size(11.0)
                                .color(SOFT),
                        );
                        ui.add(
                            egui::TextEdit::multiline(&mut self.guide_text)
                                .desired_rows(5)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.add_space(8.0);
                        if action_button(ui, "候補を生成") {
                            self.build_plan();
                        }
                    });
                });
                ui.add_space(18.0);
                panel(
                    ui,
                    "03 / COMPOSITION — 選択順がそのまま manifest になります",
                    |ui| {
                        let Some(plan) = &self.plan else {
                            ui.add_space(32.0);
                            ui.centered_and_justified(|ui| {
                                ui.label(RichText::new("候補はここに表示されます").color(SOFT))
                            });
                            return;
                        };
                        for slot in &plan.slots {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:02}", slot.slot.order))
                                        .color(MIKU)
                                        .strong(),
                                );
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&slot.slot.role)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                    ui.label(
                                        RichText::new(&slot.slot.query).size(11.0).color(SOFT),
                                    );
                                });
                                ui.add_space(16.0);
                                if slot.candidates.is_empty() {
                                    ui.label(
                                        RichText::new("候補なし")
                                            .color(Color32::from_rgb(255, 133, 151)),
                                    );
                                } else {
                                    let selected = self
                                        .selections
                                        .entry(slot.slot.id.clone())
                                        .or_insert_with(|| slot.candidates[0].segment_id.clone());
                                    egui::ComboBox::from_id_salt(&slot.slot.id)
                                        .selected_text(selected.as_str())
                                        .width(190.0)
                                        .show_ui(ui, |ui| {
                                            for candidate in &slot.candidates {
                                                ui.selectable_value(
                                                    selected,
                                                    candidate.segment_id.clone(),
                                                    format!(
                                                        "{}  ·  {}pt",
                                                        candidate.segment_id, candidate.score
                                                    ),
                                                );
                                            }
                                        });
                                    if let Some(candidate) = slot
                                        .candidates
                                        .iter()
                                        .find(|candidate| candidate.segment_id == *selected)
                                    {
                                        ui.label(
                                            RichText::new(format!(
                                                "{}  {}–{} ms",
                                                candidate.source_path,
                                                candidate.start_ms,
                                                candidate.end_ms
                                            ))
                                            .size(11.0)
                                            .color(SOFT),
                                        );
                                        ui.label(
                                            RichText::new(candidate.reasons.join(" / "))
                                                .size(10.0)
                                                .color(MIKU),
                                        );
                                    }
                                }
                            });
                            ui.add_space(10.0);
                            ui.separator();
                            ui.add_space(8.0);
                        }
                        ui.add_space(6.0);
                        if action_button(ui, "MANIFEST を保存") {
                            self.save_manifest();
                        }
                    },
                );
            });
    }
}

fn configure_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = INK;
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.selection.bg_fill = MIKU_DIM;
    visuals.selection.stroke = Stroke::new(1.0_f32, MIKU);
    visuals.widgets.active.bg_fill = MIKU_DIM;
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(25, 67, 68);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(23, 43, 55);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    ctx.set_visuals(visuals);
}
fn panel(ui: &mut egui::Ui, title: &str, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, MIKU_DIM))
        .corner_radius(6)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.label(RichText::new(title).size(12.0).strong().color(MIKU));
            ui.add_space(12.0);
            contents(ui);
        });
}
fn action_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add_sized(
        Vec2::new(180.0, 30.0),
        egui::Button::new(RichText::new(label).strong().color(INK)).fill(MIKU),
    )
    .clicked()
}
