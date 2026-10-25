use std::{collections::BTreeMap, path::PathBuf};

use eframe::egui::{self, Color32, Frame, RichText, Sense, Stroke, Ui, Vec2};

use crate::{
    domain::{
        service::{create_plan, manifest_from_plan},
        valueobject::{Candidate, Catalog, Plan, SCHEMA_VERSION, Segment, SlotPlan},
    },
    infra::catalog_file::{load_catalog, save_manifest},
};

use super::theme;

const DEMO_GUIDE: &str = "導入: テッカマン\nつなぎ: 驚き 反応";
const DEMO_CATALOG_LABEL: &str = "組み込みデモ — 素材なしで候補選択と manifest 保存を試せます";

pub struct StudioApp {
    catalog: Option<Catalog>,
    catalog_path: Option<PathBuf>,
    guide_text: String,
    plan: Option<Plan>,
    selections: BTreeMap<String, String>,
    active_slot: Option<usize>,
    status: String,
}

impl StudioApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation_context.egui_ctx);
        Self {
            catalog: Some(demo_catalog()),
            catalog_path: None,
            guide_text: DEMO_GUIDE.into(),
            plan: None,
            selections: BTreeMap::new(),
            active_slot: None,
            status: "DEMO READY — 候補選択から manifest 保存まで試せます。".into(),
        }
    }

    fn open_catalog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("catalog JSON", &["json"])
            .pick_file()
        else {
            return;
        };
        match load_catalog(&path) {
            Ok(catalog) => {
                self.catalog_path = Some(path);
                self.catalog = Some(catalog);
                self.plan = None;
                self.selections.clear();
                self.active_slot = None;
                self.status = "CATALOG READY — ガイドから候補を生成できます。".into();
            }
            Err(error) => self.status = format!("CATALOG ERROR — {error}"),
        }
    }

    fn use_demo_catalog(&mut self) {
        self.catalog = Some(demo_catalog());
        self.catalog_path = None;
        self.guide_text = DEMO_GUIDE.into();
        self.plan = None;
        self.selections.clear();
        self.active_slot = None;
        self.status = "DEMO READY — 候補選択から manifest 保存まで試せます。".into();
    }

    fn build_plan(&mut self) {
        let Some(catalog) = &self.catalog else {
            self.status = "CATALOG JSON を先に読み込んでください。".into();
            return;
        };
        let plan = create_plan(catalog, &self.guide_text);
        if plan.slots.is_empty() {
            self.status = "GUIDE を `役割: 語句 タグ` の形式で入力してください。".into();
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
        self.active_slot = Some(0);
        self.status = format!(
            "{} SLOTS READY — 素材候補を選択して STAGE を確定します。",
            plan.slots.len()
        );
        self.plan = Some(plan);
    }

    fn save_manifest(&mut self) {
        let Some(plan) = &self.plan else {
            self.status = "候補を生成してから保存してください。".into();
            return;
        };
        let dialog = rfd::FileDialog::new()
            .set_file_name("composition-manifest.json")
            .set_directory(manifest_save_directory());
        let Some(path) = dialog.save_file() else {
            return;
        };
        match manifest_from_plan(plan, &self.selections)
            .map_err(|error| error.to_string())
            .and_then(|manifest| save_manifest(&path, &manifest).map_err(|error| error.to_string()))
        {
            Ok(()) => self.status = format!("MANIFEST SAVED — {}", path.display()),
            Err(error) => self.status = format!("MANIFEST ERROR — {error}"),
        }
    }

    fn active_slot_plan(&self) -> Option<&SlotPlan> {
        self.plan
            .as_ref()?
            .slots
            .get(self.active_slot.unwrap_or_default())
    }

    fn show_header(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("CUTUP")
                    .size(25.0)
                    .strong()
                    .color(theme::ACCENT),
            );
            ui.label(RichText::new("STUDIO").size(25.0).strong());
            if self.catalog_path.is_none() {
                theme::badge(ui, "DEMO", theme::AMBER);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::badge(ui, "MANIFEST", theme::ACCENT);
                ui.label(
                    RichText::new(format!("SCHEMA V{SCHEMA_VERSION}"))
                        .monospace()
                        .size(10.0)
                        .color(theme::MUTED),
                );
            });
        });
        ui.label(
            RichText::new("ガイドを読む。素材を選ぶ。構成を残す。")
                .size(12.0)
                .color(theme::MUTED),
        );
    }

    fn show_catalog_controls(&mut self, ui: &mut Ui) {
        theme::panel().show(ui, |ui| {
            theme::caption(ui, "01 / MATERIAL CATALOG");
            let location = self
                .catalog_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| DEMO_CATALOG_LABEL.into());
            ui.add(
                egui::Label::new(RichText::new(location).size(11.0).color(theme::MUTED)).truncate(),
            );
            ui.add_space(4.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("OPEN SOURCE CATALOG JSON")
                            .strong()
                            .color(theme::BACKGROUND),
                    )
                    .fill(theme::ACCENT)
                    .min_size(Vec2::new(190.0, 32.0)),
                )
                .clicked()
            {
                self.open_catalog();
            }
            if self.catalog_path.is_some() && ui.button("RESET TO DEMO CATALOG").clicked() {
                self.use_demo_catalog();
            }
            ui.label(
                RichText::new("保存した composition-manifest.json はここには読み込めません。")
                    .size(10.0)
                    .color(theme::MUTED),
            );
            if let Some(catalog) = &self.catalog {
                ui.horizontal(|ui| {
                    let label = if self.catalog_path.is_none() {
                        "候補用のデモ素材"
                    } else {
                        "読み込んだ素材区間"
                    };
                    ui.label(
                        RichText::new(format!("{label}: {} 件", catalog.segments.len()))
                            .size(10.0)
                            .color(theme::MUTED),
                    );
                    if self.catalog_path.is_some() {
                        ui.label(
                            RichText::new(&catalog.material_root)
                                .size(10.0)
                                .color(theme::MUTED),
                        );
                    }
                });
            }
        });
    }

    fn show_guide_controls(&mut self, ui: &mut Ui) {
        theme::panel().show(ui, |ui| {
            theme::caption(ui, "02 / COMPOSITION GUIDE");
            ui.label(
                RichText::new("一行ごとに  役割: 探したい語句 タグ")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.add(
                egui::TextEdit::multiline(&mut self.guide_text)
                    .desired_rows(4)
                    .font(egui::TextStyle::Monospace),
            );
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("GENERATE CANDIDATES")
                            .strong()
                            .color(theme::BACKGROUND),
                    )
                    .fill(theme::ACCENT)
                    .min_size(Vec2::new(220.0, 32.0)),
                )
                .clicked()
            {
                self.build_plan();
            }
        });
    }

    fn show_slot_tabs(&mut self, ui: &mut Ui) {
        let Some(plan) = &self.plan else {
            return;
        };
        theme::caption(ui, "03 / GUIDE SLOTS");
        ui.horizontal_wrapped(|ui| {
            for (index, slot) in plan.slots.iter().enumerate() {
                let selected = self.active_slot == Some(index);
                let label = format!("{:02} {}", slot.slot.order, slot.slot.role);
                if ui.selectable_label(selected, label).clicked() {
                    self.active_slot = Some(index);
                }
            }
        });
    }

    fn show_candidate_grid(&mut self, ui: &mut Ui) {
        let Some(slot) = self.active_slot_plan() else {
            theme::panel().show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("候補はここに表示されます").color(theme::MUTED));
                });
            });
            return;
        };
        let slot_id = slot.slot.id.clone();
        let heading = format!("04 / CANDIDATES — {} / {}", slot.slot.role, slot.slot.query);
        let candidates = slot.candidates.clone();
        let selected = self.selections.get(&slot_id).cloned();
        theme::panel().show(ui, |ui| {
            theme::caption(ui, &heading);
            if candidates.is_empty() {
                ui.add_space(18.0);
                ui.colored_label(
                    theme::RED,
                    "候補がありません。ガイドの語句・タグ・役割を確認してください。",
                );
                return;
            }
            let card_width = ((ui.available_width() - 12.0) / 2.0).max(180.0);
            let mut chosen = None;
            for row in candidates.chunks(2) {
                ui.horizontal_top(|ui| {
                    for candidate in row {
                        if candidate_card(
                            ui,
                            candidate,
                            selected.as_deref() == Some(candidate.segment_id.as_str()),
                            card_width,
                        ) {
                            chosen = Some(candidate.segment_id.clone());
                        }
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(segment_id) = chosen {
                self.selections.insert(slot_id, segment_id);
                self.status = "CANDIDATE SELECTED — STAGE の順序を確認して保存できます。".into();
            }
        });
    }

    fn show_stage(&mut self, ui: &mut Ui) {
        let entries: Vec<_> = self
            .plan
            .as_ref()
            .map(|plan| {
                plan.slots
                    .iter()
                    .map(|slot| {
                        let selected_id = self.selections.get(&slot.slot.id);
                        let candidate = selected_id.and_then(|id| {
                            slot.candidates
                                .iter()
                                .find(|candidate| candidate.segment_id == *id)
                        });
                        (slot.slot.order, slot.slot.role.clone(), candidate.cloned())
                    })
                    .collect()
            })
            .unwrap_or_default();
        theme::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::caption(ui, "COMPOSITION STAGE");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::badge(ui, &format!("{} CUTS", entries.len()), theme::ACCENT);
                });
            });
            ui.label(
                RichText::new("確定する順序。映像・音声の書き出しは行いません。")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.add_space(8.0);
            if entries.is_empty() {
                ui.add_space(24.0);
                ui.horizontal_centered(|ui| {
                    ui.label(
                        RichText::new("GUIDE を生成すると構成が表示されます").color(theme::MUTED),
                    );
                });
                ui.add_space(24.0);
            }
            for (order, role, candidate) in &entries {
                let (name, timing, source) = candidate.as_ref().map_or_else(
                    || {
                        (
                            "UNSELECTED".to_owned(),
                            "--".to_owned(),
                            "候補なし".to_owned(),
                        )
                    },
                    |candidate| {
                        (
                            candidate.segment_id.clone(),
                            format!("{}–{} ms", candidate.start_ms, candidate.end_ms),
                            candidate.source_path.clone(),
                        )
                    },
                );
                Frame::new()
                    .fill(Color32::from_rgb(18, 39, 43))
                    .stroke(Stroke::new(1.0_f32, theme::BORDER))
                    .corner_radius(6)
                    .inner_margin(10)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            theme::badge(ui, &format!("{:02}", order), theme::ACCENT);
                            ui.vertical(|ui| {
                                ui.label(RichText::new(name).size(12.0).strong());
                                ui.label(
                                    RichText::new(format!("{} / {}", role, timing))
                                        .size(10.0)
                                        .color(theme::MUTED),
                                );
                                ui.label(RichText::new(source).size(10.0).color(theme::MUTED));
                            });
                        });
                    });
                ui.add_space(3.0);
            }
            ui.add_space(8.0);
            if ui
                .add_enabled(
                    !entries.is_empty(),
                    egui::Button::new(
                        RichText::new("SAVE MANIFEST")
                            .strong()
                            .color(theme::BACKGROUND),
                    )
                    .fill(theme::ACCENT)
                    .min_size(Vec2::new(180.0, 34.0)),
                )
                .clicked()
            {
                self.save_manifest();
            }
        });
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("status").show(context, |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, "SYSTEM", theme::AMBER);
                ui.label(RichText::new(&self.status).size(11.0).color(theme::MUTED));
            });
        });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BACKGROUND).inner_margin(20))
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_header(ui);
                    ui.add_space(12.0);
                    ui.columns(2, |columns| {
                        let controls = &mut columns[0];
                        self.show_catalog_controls(controls);
                        controls.add_space(8.0);
                        self.show_guide_controls(controls);
                        controls.add_space(10.0);
                        self.show_slot_tabs(controls);
                        controls.add_space(6.0);
                        self.show_candidate_grid(controls);

                        let stage = &mut columns[1];
                        self.show_stage(stage);
                    });
                });
            });
    }
}

fn manifest_save_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|profile| profile.join("Downloads"))
        .filter(|directory| directory.is_dir())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn demo_catalog() -> Catalog {
    Catalog {
        schema_version: SCHEMA_VERSION,
        material_root: "組み込みデモ（素材ファイルなし）".into(),
        segments: vec![
            Segment {
                id: "demo-tekkaman-intro".into(),
                source_path: "demo/tekkaman-intro.mp4".into(),
                start_ms: 1200,
                end_ms: 4500,
                transcript: "テッカマンが登場する導入シーン".into(),
                keywords: vec!["テッカマン".into(), "登場".into()],
                tags: vec!["導入".into(), "画".into()],
                roles: vec!["導入".into()],
                note: "動作確認用の仮想素材".into(),
            },
            Segment {
                id: "demo-tekkaman-closeup".into(),
                source_path: "demo/tekkaman-closeup.mp4".into(),
                start_ms: 8000,
                end_ms: 10500,
                transcript: "テッカマンのクローズアップ".into(),
                keywords: vec!["テッカマン".into(), "クローズアップ".into()],
                tags: vec!["導入".into(), "画".into()],
                roles: vec!["導入".into()],
                note: "動作確認用の仮想素材".into(),
            },
            Segment {
                id: "demo-surprised-reaction".into(),
                source_path: "demo/surprised-reaction.mp4".into(),
                start_ms: 8400,
                end_ms: 10400,
                transcript: "えっ、という驚きの反応".into(),
                keywords: vec!["驚き".into(), "反応".into()],
                tags: vec!["反応".into(), "つなぎ".into()],
                roles: vec!["つなぎ".into()],
                note: "動作確認用の仮想素材".into(),
            },
            Segment {
                id: "demo-quick-reaction".into(),
                source_path: "demo/quick-reaction.mp4".into(),
                start_ms: 2000,
                end_ms: 3600,
                transcript: "短い驚きのリアクション".into(),
                keywords: vec!["驚き".into(), "反応".into()],
                tags: vec!["反応".into(), "つなぎ".into()],
                roles: vec!["つなぎ".into()],
                note: "動作確認用の仮想素材".into(),
            },
        ],
    }
}

fn candidate_card(ui: &mut Ui, candidate: &Candidate, selected: bool, width: f32) -> bool {
    let response = Frame::new()
        .fill(if selected {
            Color32::from_rgb(24, 48, 49)
        } else {
            theme::PANEL
        })
        .stroke(Stroke::new(
            1.0_f32,
            if selected {
                theme::ACCENT
            } else {
                theme::BORDER
            },
        ))
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            ui.horizontal(|ui| {
                theme::badge(
                    ui,
                    &format!("{} PT", candidate.score),
                    if selected {
                        theme::ACCENT
                    } else {
                        theme::MUTED
                    },
                );
                if selected {
                    ui.label(
                        RichText::new("SELECTED")
                            .size(10.0)
                            .strong()
                            .color(theme::ACCENT),
                    );
                }
            });
            ui.label(RichText::new(&candidate.segment_id).size(12.0).strong());
            ui.label(
                RichText::new(format!("{}–{} ms", candidate.start_ms, candidate.end_ms))
                    .monospace()
                    .size(10.0)
                    .color(theme::MUTED),
            );
            ui.add(
                egui::Label::new(
                    RichText::new(&candidate.source_path)
                        .size(10.0)
                        .color(theme::MUTED),
                )
                .truncate(),
            );
            ui.label(
                RichText::new(candidate.reasons.join(" / "))
                    .size(10.0)
                    .color(theme::ACCENT),
            );
        })
        .response;
    let response = ui.interact(
        response.rect,
        ui.id().with((&candidate.segment_id, candidate.start_ms)),
        Sense::click(),
    );
    if selected || response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            8.0,
            Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_catalog_generates_candidates_for_the_default_guide() {
        let catalog = demo_catalog();
        catalog.validate_metadata().unwrap();

        let plan = create_plan(&catalog, DEMO_GUIDE);

        assert_eq!(plan.slots.len(), 2);
        assert!(plan.slots.iter().all(|slot| !slot.candidates.is_empty()));
    }
}
