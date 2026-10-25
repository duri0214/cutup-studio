use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use eframe::egui::{self, Color32, Frame, RichText, Sense, Stroke, Ui, Vec2};

use crate::{
    domain::{
        service::{create_plan, manifest_from_plan},
        valueobject::{Candidate, Catalog, Plan, SCHEMA_VERSION, Segment},
    },
    infra::catalog_file::{load_catalog, save_manifest},
};

use super::theme;

#[derive(Clone)]
struct GuideInput {
    role: String,
    query: String,
}

pub struct StudioApp {
    catalog: Option<Catalog>,
    catalog_path: Option<PathBuf>,
    guide_inputs: Vec<GuideInput>,
    plan: Option<Plan>,
    selections: BTreeMap<String, String>,
    active_slot: Option<usize>,
    dragged_guide_index: Option<usize>,
    status: String,
}

impl StudioApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        theme::install(&creation_context.egui_ctx);
        Self {
            catalog: Some(demo_catalog()),
            catalog_path: None,
            guide_inputs: demo_guide_inputs(),
            plan: None,
            selections: BTreeMap::new(),
            active_slot: None,
            dragged_guide_index: None,
            status: "READY — GUIDE を生成して候補を確認できます。".into(),
        }
    }

    fn open_catalog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("catalog JSON を選択")
            .add_filter("catalog JSON（素材フォルダと素材クリップ）", &["json"])
            .pick_file()
        else {
            return;
        };
        match load_catalog(&path) {
            Ok(catalog) => {
                self.catalog_path = Some(path);
                self.catalog = Some(catalog);
                self.normalize_guide_roles();
                self.plan = None;
                self.selections.clear();
                self.active_slot = None;
                self.dragged_guide_index = None;
                self.status = "CATALOG READY — ガイドから候補を生成できます。".into();
            }
            Err(error) => self.status = format!("CATALOG ERROR — {error}"),
        }
    }

    fn use_demo_catalog(&mut self) {
        self.catalog = Some(demo_catalog());
        self.catalog_path = None;
        self.guide_inputs = demo_guide_inputs();
        self.plan = None;
        self.selections.clear();
        self.active_slot = None;
        self.dragged_guide_index = None;
        self.status = "READY — GUIDE を生成して候補を確認できます。".into();
    }

    fn build_plan(&mut self) {
        let guide_text = self.guide_text();
        let Some(catalog) = &self.catalog else {
            self.status = "CATALOG JSON を先に読み込んでください。".into();
            return;
        };
        let plan = create_plan(catalog, &guide_text);
        if plan.slots.is_empty() {
            self.status = "GUIDE に語句またはタグを入力してください。".into();
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

    fn available_roles(&self) -> Vec<String> {
        self.catalog
            .iter()
            .flat_map(|catalog| catalog.segments.iter())
            .flat_map(|segment| segment.roles.iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn normalize_guide_roles(&mut self) {
        let available_roles = self.available_roles();
        let Some(first_role) = available_roles.first() else {
            return;
        };
        for guide_input in &mut self.guide_inputs {
            if !available_roles.contains(&guide_input.role) {
                guide_input.role.clone_from(first_role);
            }
        }
    }

    fn guide_text(&self) -> String {
        guide_text_from_inputs(&self.guide_inputs)
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

    fn show_header(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            ui.label(
                RichText::new("CUTUP")
                    .size(25.0)
                    .strong()
                    .color(theme::ACCENT),
            );
            ui.label(RichText::new("STUDIO").size(25.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::badge(ui, "MANIFEST", theme::ACCENT);
                if self.catalog_path.is_none() {
                    theme::badge(ui, "DEMO", theme::AMBER);
                }
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
            ui.label(
                RichText::new(
                    "実素材を使う場合は、素材フォルダと素材クリップを定義した catalog JSON を開きます。",
                )
                .size(11.0)
                .color(theme::MUTED),
            );
            ui.add_space(4.0);
            if let Some(path) = &self.catalog_path {
                ui.add(
                    egui::Label::new(
                        RichText::new(path.display().to_string())
                            .size(11.0)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
                ui.add_space(4.0);
            }
            if theme::action_button(
                ui,
                theme::ActionIcon::Open,
                "OPEN",
                false,
                Vec2::new(106.0, 32.0),
            )
            .clicked()
            {
                self.open_catalog();
            }
            if self.catalog_path.is_some()
                && theme::action_button(
                    ui,
                    theme::ActionIcon::Reset,
                    "RESET TO DEMO CATALOG",
                    false,
                    Vec2::new(250.0, 32.0),
                )
                .clicked()
            {
                self.use_demo_catalog();
            }
            if let Some(catalog) = &self.catalog {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "登録済みの素材クリップ: {} 件",
                            catalog.segments.len()
                        ))
                        .size(10.0)
                        .color(theme::MUTED),
                    );
                    if self.catalog_path.is_some() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("素材フォルダ: {}", catalog.material_root))
                                    .size(10.0)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    }
                });
            }
        });
    }

    fn show_guide_controls(&mut self, ui: &mut Ui) {
        let available_roles = self.available_roles();
        let mut role_to_add = None;
        theme::panel().show(ui, |ui| {
            theme::caption(ui, "02 / COMPOSITION GUIDE");
            ui.label(
                RichText::new("役割を選び、探したい語句・タグを入力します。")
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.horizontal_wrapped(|ui| {
                theme::caption(ui, "ADD SLOT");
                for role in &available_roles {
                    if theme::badge_button(ui, role, theme::ACCENT).clicked() {
                        role_to_add = Some(role.clone());
                    }
                }
            });
            ui.add_space(4.0);
            let mut remove_index = None;
            let mut drag_started = None;
            let mut drag_stopped = false;
            let mut drop_target = None;
            let dragged_index = self.dragged_guide_index;
            let can_remove = self.guide_inputs.len() > 1;
            for (index, guide_input) in self.guide_inputs.iter_mut().enumerate() {
                let row = ui.horizontal(|ui| {
                    let drag_response = theme::drag_handle(ui);
                    if drag_response.drag_started() {
                        drag_started = Some(index);
                    }
                    drag_stopped |= drag_response.drag_stopped();
                    theme::badge(ui, &guide_input.role, theme::ACCENT);
                    ui.add(
                        egui::TextEdit::singleline(&mut guide_input.query)
                            .desired_width(330.0)
                            .hint_text("探したい語句・タグ"),
                    );
                    if can_remove
                        && theme::icon_button(ui, theme::ActionIcon::Delete, "このスロットを削除")
                            .clicked()
                    {
                        remove_index = Some(index);
                    }
                });
                if let Some(dragged_index) = dragged_index
                    && dragged_index != index
                    && ui.input(|input| {
                        input
                            .pointer
                            .interact_pos()
                            .is_some_and(|position| row.response.rect.contains(position))
                    })
                {
                    drop_target = Some(index);
                    ui.painter().line_segment(
                        [row.response.rect.left_top(), row.response.rect.right_top()],
                        Stroke::new(2.0_f32, theme::ACCENT),
                    );
                }
            }
            if let Some(index) = remove_index {
                self.guide_inputs.remove(index);
            }
            if let Some(index) = drag_started {
                self.dragged_guide_index = Some(index);
            }
            if drag_stopped {
                if let (Some(from), Some(to)) = (self.dragged_guide_index, drop_target)
                    && from != to
                {
                    let guide_input = self.guide_inputs.remove(from);
                    self.guide_inputs.insert(to, guide_input);
                }
                self.dragged_guide_index = None;
            }
            if let Some(role) = role_to_add {
                self.guide_inputs.push(GuideInput {
                    role,
                    query: String::new(),
                });
            }
            if theme::action_button(
                ui,
                theme::ActionIcon::Generate,
                "GENERATE CANDIDATES",
                true,
                Vec2::new(250.0, 32.0),
            )
            .clicked()
            {
                self.build_plan();
            }
        });
    }

    fn show_candidate_grid(&mut self, ui: &mut Ui) {
        let slot_tabs: Vec<_> = self
            .plan
            .as_ref()
            .map(|plan| {
                plan.slots
                    .iter()
                    .map(|slot| format!("{:02} {}", slot.slot.order, slot.slot.role))
                    .collect()
            })
            .unwrap_or_default();
        let active_index = self.active_slot.unwrap_or_default();
        let active_slot = self
            .plan
            .as_ref()
            .and_then(|plan| plan.slots.get(active_index));
        let Some(slot) = active_slot else {
            theme::panel().show(ui, |ui| {
                theme::caption(ui, "03 / CANDIDATES");
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("候補はここに表示されます").color(theme::MUTED));
                });
            });
            return;
        };
        let slot_id = slot.slot.id.clone();
        let candidates = slot.candidates.clone();
        let selected = self.selections.get(&slot_id).cloned();
        let mut next_active_index = active_index;
        theme::panel().show(ui, |ui| {
            theme::caption(ui, "03 / CANDIDATES");
            ui.horizontal_wrapped(|ui| {
                for (index, label) in slot_tabs.iter().enumerate() {
                    if ui.selectable_label(index == active_index, label).clicked() {
                        next_active_index = index;
                    }
                }
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("{} / {}", slot.slot.role, slot.slot.query))
                    .size(11.0)
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);
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
        if next_active_index != active_index {
            self.active_slot = Some(next_active_index);
            ui.ctx().request_repaint();
        }
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
                ui.set_min_height(32.0);
                theme::caption(ui, "COMPOSITION STAGE");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    theme::badge(ui, &format!("{} CUTS", entries.len()), theme::ACCENT);
                });
            });
            ui.label(
                RichText::new(
                    "確定する順序。SAVE MANIFEST で composition-manifest.json を Downloads に出力します。",
                )
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
            let save_response = ui.add_enabled_ui(!entries.is_empty(), |ui| {
                theme::action_button(
                    ui,
                    theme::ActionIcon::Save,
                    "SAVE MANIFEST",
                    true,
                    Vec2::new(210.0, 34.0),
                )
            });
            if save_response.inner.clicked() {
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

fn demo_guide_inputs() -> Vec<GuideInput> {
    vec![
        GuideInput {
            role: "導入".into(),
            query: "テッカマン".into(),
        },
        GuideInput {
            role: "つなぎ".into(),
            query: "驚き 反応".into(),
        },
    ]
}

fn guide_text_from_inputs(guide_inputs: &[GuideInput]) -> String {
    guide_inputs
        .iter()
        .filter(|guide_input| !guide_input.query.trim().is_empty())
        .map(|guide_input| format!("{}: {}", guide_input.role, guide_input.query.trim()))
        .collect::<Vec<_>>()
        .join("\n")
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
            ui.vertical(|ui| {
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
                ui.add(
                    egui::Label::new(RichText::new(&candidate.segment_id).size(12.0).strong())
                        .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{}–{} ms", candidate.start_ms, candidate.end_ms))
                            .monospace()
                            .size(10.0)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(&candidate.source_path)
                            .size(10.0)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(candidate.reasons.join(" / "))
                            .size(10.0)
                            .color(theme::ACCENT),
                    )
                    .truncate(),
                );
            });
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

        let plan = create_plan(&catalog, &guide_text_from_inputs(&demo_guide_inputs()));

        assert_eq!(plan.slots.len(), 2);
        assert!(plan.slots.iter().all(|slot| !slot.candidates.is_empty()));
    }
}
