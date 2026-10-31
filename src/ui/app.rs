use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, Color32, Frame, RichText, Sense, Stroke, Ui, Vec2};

use crate::{
    domain::valueobject::catalog::ScanReport,
    domain::{
        service::{create_plan, manifest_from_plan},
        valueobject::{Candidate, Catalog, Plan, SCHEMA_VERSION},
    },
    infra::catalog_file::save_manifest,
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
};

use super::catalog_editor::CatalogEditor;
use super::theme;

type CatalogTask = Receiver<Result<(SqliteCatalog, PathBuf, ScanReport), CatalogError>>;

#[derive(Clone)]
struct GuideInput {
    role: String,
    query: String,
}

pub struct StudioApp {
    catalog: Option<Catalog>,
    catalog_path: Option<PathBuf>,
    demo_mode: bool,
    editor: Option<CatalogEditor>,
    catalog_task: Option<CatalogTask>,
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
            catalog: None,
            catalog_path: None,
            demo_mode: false,
            editor: None,
            catalog_task: None,
            guide_inputs: Vec::new(),
            plan: None,
            selections: BTreeMap::new(),
            active_slot: None,
            dragged_guide_index: None,
            status: "NO CATALOG — NEW または OPEN で基準フォルダを選んでください。".into(),
        }
    }

    fn open_catalog(&mut self, context: &egui::Context) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("基準フォルダを選択")
            .pick_folder()
        else {
            return;
        };
        let path = root.join("material-catalog.sqlite");
        match path.try_exists() {
            Ok(true) => self.start_catalog_task(path, None, context),
            Ok(false) => {
                self.clear_active_catalog();
                self.status = format!(
                    "CATALOG NOT FOUND — {} に material-catalog.sqlite がありません。NEW で作成してください。",
                    root.display()
                );
            }
            Err(error) => {
                self.clear_active_catalog();
                self.status = format!("CATALOG ERROR — 基準フォルダを確認できません: {error}");
            }
        }
    }

    fn clear_active_catalog(&mut self) {
        self.catalog = None;
        self.catalog_path = None;
        self.demo_mode = false;
        self.editor = None;
        self.plan = None;
        self.selections.clear();
        self.active_slot = None;
        self.dragged_guide_index = None;
    }

    fn use_demo_catalog(&mut self, context: &egui::Context) {
        let root = demo_material_root();
        let path = root.join("material-catalog.sqlite");
        match path.try_exists() {
            Ok(true) => self.start_catalog_task(path, None, context),
            Ok(false) => self.start_catalog_task(path, Some(root), context),
            Err(error) => {
                self.clear_active_catalog();
                self.status = format!("CATALOG ERROR — DEMO のカタログを確認できません: {error}");
            }
        }
        self.demo_mode = true;
        self.guide_inputs.clear();
    }

    fn is_demo_catalog(&self) -> bool {
        self.demo_mode
    }

    fn create_catalog(&mut self, context: &egui::Context) {
        let Some(root) = rfd::FileDialog::new()
            .set_title("基準フォルダを選択（materials フォルダが必要）")
            .pick_folder()
        else {
            return;
        };
        let path = root.join("material-catalog.sqlite");
        match path.try_exists() {
            Ok(true) => {
                self.clear_active_catalog();
                self.status = format!(
                    "CATALOG EXISTS — {} はすでにあります。OPEN で基準フォルダを開いてください。",
                    path.display()
                );
            }
            Ok(false) => self.start_catalog_task(path, Some(root), context),
            Err(error) => {
                self.clear_active_catalog();
                self.status = format!("CATALOG ERROR — 保存先を確認できません: {error}");
            }
        }
    }

    fn start_catalog_task(
        &mut self,
        path: PathBuf,
        root: Option<PathBuf>,
        context: &egui::Context,
    ) {
        self.clear_active_catalog();
        let (sender, receiver) = mpsc::channel();
        let context = context.clone();
        std::thread::spawn(move || {
            let result = (|| {
                let mut store = match root {
                    Some(root) => SqliteCatalog::create(&path, &root)?,
                    None => SqliteCatalog::open(&path)?,
                };
                let report = store.rescan()?;
                Ok((store, path, report))
            })();
            let _ = sender.send(result);
            context.request_repaint();
        });
        self.catalog_task = Some(receiver);
        self.status =
            "SCANNING — 素材を登録しています。ファイル数・サイズにより時間がかかります。".into();
    }

    fn poll_catalog_task(&mut self) {
        let Some(task) = &self.catalog_task else {
            return;
        };
        let result = match task.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.catalog_task = None;
                self.clear_active_catalog();
                self.status =
                    "CATALOG ERROR — フォルダ走査が中断されました。再走査してください。".into();
                return;
            }
        };
        self.catalog_task = None;
        match result
            .and_then(|(store, path, report)| Ok((CatalogEditor::new(store)?, path, report)))
        {
            Ok((mut editor, path, report)) => {
                let (total, _) = editor.counts();
                editor.open = total > 0;
                self.editor = Some(editor);
                self.catalog_path = Some(path);
                self.reload_catalog();
                self.status = if total == 0 {
                    "SCAN COMPLETE — 素材ファイルが見つかりません。素材をフォルダへ入れて RESCAN してください。".into()
                } else {
                    format!(
                        "SCAN COMPLETE — 追加 {} / 変更 {} / 利用不能 {} / 復帰 {}。素材の情報を登録して GUIDE から候補を生成できます。",
                        report.added, report.changed, report.missing, report.restored
                    )
                };
            }
            Err(error) => {
                // A failed rescan must not leave an old plan available for export.
                self.clear_active_catalog();
                self.status = format!("CATALOG ERROR — {error}");
            }
        }
    }

    fn reload_catalog(&mut self) {
        let result = self
            .editor
            .as_ref()
            .map(|editor| editor.store.candidate_catalog());
        if let Some(result) = result {
            match result {
                Ok(catalog) => self.catalog = Some(catalog),
                Err(error) => {
                    self.clear_active_catalog();
                    self.status = format!("CATALOG ERROR — {error}");
                    return;
                }
            }
        }
        self.plan = None;
        self.selections.clear();
        self.active_slot = None;
        self.normalize_guide_roles();
    }

    fn build_plan(&mut self) {
        let guide_text = self.guide_text();
        let Some(catalog) = &self.catalog else {
            self.status = "基準フォルダを開くか、materials フォルダを再走査してください。".into();
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
                    .map(|candidate| (slot.slot.id.clone(), candidate.material_id.clone()))
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
            .flat_map(|catalog| catalog.materials.iter())
            .flat_map(|material| material.roles.iter().cloned())
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
            .set_directory(&plan.material_root);
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

    fn system_status(&self) -> (theme::Signal, Color32, &'static str) {
        if self.status.starts_with("CATALOG ERROR")
            || self.status.starts_with("CATALOG EXISTS")
            || self.status.starts_with("MANIFEST ERROR")
        {
            (theme::Signal::Error, theme::RED, "ERROR")
        } else if self.catalog_task.is_some() {
            (theme::Signal::Attention, theme::AMBER, "SCANNING")
        } else if self.catalog.is_none() {
            (theme::Signal::Attention, theme::AMBER, "NO CATALOG")
        } else if self.is_demo_catalog() {
            (theme::Signal::Ready, theme::ACCENT, "DEMO")
        } else {
            (theme::Signal::Ready, theme::ACCENT, "READY")
        }
    }

    fn guide_status(&self) -> (theme::Signal, &'static str) {
        if self.catalog.is_none() {
            (
                theme::Signal::Attention,
                "01 の素材カタログを開いてください",
            )
        } else if self.plan.is_some() {
            (theme::Signal::Ready, "GUIDE から候補を生成済みです")
        } else {
            (
                theme::Signal::Attention,
                "GUIDE を入力して候補を生成してください",
            )
        }
    }

    fn candidates_status(&self) -> (theme::Signal, &'static str) {
        if self.catalog.is_none() {
            (
                theme::Signal::Attention,
                "01 の素材カタログを開いてください",
            )
        } else if let Some(plan) = &self.plan {
            let active_index = self.active_slot.unwrap_or_default();
            match plan.slots.get(active_index) {
                Some(slot) if slot.candidates.is_empty() => (
                    theme::Signal::Error,
                    "候補がありません。GUIDE の語句・タグ・役割を確認してください",
                ),
                Some(_) => (theme::Signal::Ready, "候補を選択できます"),
                None => (theme::Signal::Attention, "02 で GUIDE を生成してください"),
            }
        } else {
            (theme::Signal::Attention, "02 で GUIDE を生成してください")
        }
    }

    fn show_header(&self, ui: &mut Ui) {
        let (signal, _, _) = self.system_status();
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
                if self.is_demo_catalog() && signal != theme::Signal::Error {
                    theme::badge(ui, "DEMO", theme::ACCENT);
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
            RichText::new("基準フォルダ内の素材から、構成の指示書を作る。")
                .size(12.0)
                .color(theme::MUTED),
        );
    }

    fn show_catalog_controls(&mut self, ui: &mut Ui) {
        let (signal, _, _) = self.system_status();
        theme::panel().show(ui, |ui| {
            section_header(ui, "01 / MATERIAL CATALOG", signal, &self.status);
            ui.label(
                RichText::new(
                    "基準フォルダには materials/ と material-catalog.sqlite を置きます。",
                )
                .size(11.0)
                .color(theme::MUTED),
            );
            ui.add_space(4.0);
            ui.add_enabled_ui(self.catalog_task.is_none(), |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        if theme::action_button(
                            ui,
                            theme::ActionIcon::New,
                            "NEW",
                            false,
                            Vec2::new(106.0, 32.0),
                        )
                        .clicked()
                        {
                            self.create_catalog(ui.ctx());
                        }
                        ui.add_sized(
                            Vec2::new(106.0, 22.0),
                            egui::Label::new(
                                RichText::new("基準フォルダを選ぶ")
                                    .size(10.0)
                                    .color(theme::MUTED),
                            )
                            .wrap(),
                        );
                    });
                    ui.vertical(|ui| {
                        if theme::action_button(
                            ui,
                            theme::ActionIcon::Open,
                            "OPEN",
                            false,
                            Vec2::new(106.0, 32.0),
                        )
                        .clicked()
                        {
                            self.open_catalog(ui.ctx());
                        }
                        ui.add_sized(
                            Vec2::new(106.0, 22.0),
                            egui::Label::new(
                                RichText::new("前の基準フォルダを選ぶ")
                                    .size(10.0)
                                    .color(theme::MUTED),
                            )
                            .wrap(),
                        );
                    });
                    ui.vertical(|ui| {
                        if theme::action_button(
                            ui,
                            theme::ActionIcon::Reset,
                            "DEMO",
                            self.is_demo_catalog(),
                            Vec2::new(106.0, 32.0),
                        )
                        .clicked()
                        {
                            self.use_demo_catalog(ui.ctx());
                        }
                        ui.add_sized(
                            Vec2::new(106.0, 22.0),
                            egui::Label::new(
                                RichText::new("サンプルで試す")
                                    .size(10.0)
                                    .color(theme::MUTED),
                            )
                            .wrap(),
                        );
                    });
                });
                if let Some(editor) = &mut self.editor {
                    let (total, missing) = editor.counts();
                    let rescan = if total == 0 {
                        ui.horizontal(|ui| {
                            ui.label("次: 素材ファイルをフォルダに入れてから、更新を押します。");
                            theme::icon_button(
                                ui,
                                theme::ActionIcon::Reset,
                                "RESCAN — フォルダを再走査",
                            )
                            .clicked()
                        })
                        .inner
                    } else {
                        ui.label(format!("素材ファイル: {total} 件 / 利用不能: {missing} 件"));
                        ui.horizontal(|ui| {
                            if ui.button("次: 素材の情報を登録").clicked() {
                                editor.open = true;
                            }
                            theme::icon_button(
                                ui,
                                theme::ActionIcon::Reset,
                                "RESCAN — フォルダを再走査",
                            )
                            .clicked()
                        })
                        .inner
                    };
                    if rescan && let Some(path) = self.catalog_path.clone() {
                        self.start_catalog_task(path, None, ui.ctx());
                    }
                }
            });
            if let Some(catalog) = &self.catalog {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "情報を登録済みの素材: {} 件",
                            catalog.materials.len()
                        ))
                        .size(10.0)
                        .color(theme::MUTED),
                    );
                    if self.catalog_path.is_some() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("基準フォルダ: {}", catalog.material_root))
                                    .size(10.0)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    }
                });
            } else {
                ui.label(
                    RichText::new("次: NEW / OPEN / DEMO のどれかを選びます。")
                        .size(10.0)
                        .color(theme::MUTED),
                );
            }
        });
    }

    fn show_guide_controls(&mut self, ui: &mut Ui) {
        let available_roles = self.available_roles();
        let (signal, status) = self.guide_status();
        let mut role_to_add = None;
        theme::panel().show(ui, |ui| {
            section_header(ui, "02 / COMPOSITION GUIDE", signal, status);
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
                    theme::role_badge(ui, &guide_input.role, theme::ACCENT);
                    ui.add(
                        egui::TextEdit::singleline(&mut guide_input.query)
                            .desired_width(330.0)
                            .min_size(Vec2::new(330.0, 28.0))
                            .margin(egui::Margin::symmetric(8, 4))
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
        let (signal, status) = self.candidates_status();
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
                section_header(ui, "03 / CANDIDATES", signal, status);
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
            section_header(ui, "03 / CANDIDATES", signal, status);
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
                            selected.as_deref() == Some(candidate.material_id.as_str()),
                            card_width,
                        ) {
                            chosen = Some(candidate.material_id.clone());
                        }
                    }
                });
                ui.add_space(4.0);
            }
            if let Some(material_id) = chosen {
                self.selections.insert(slot_id, material_id);
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
                                .find(|candidate| candidate.material_id == *id)
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
                    theme::badge(ui, &format!("{} MATERIALS", entries.len()), theme::ACCENT);
                });
            });
            ui.label(
                RichText::new(
                    "選んだ素材の順番を、composition-manifest.json として保存します。素材そのものは含みません。",
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
                let (name, source) = candidate.as_ref().map_or_else(
                    || {
                        (
                            "UNSELECTED".to_owned(),
                            "候補なし".to_owned(),
                        )
                    },
                    |candidate| {
                        (
                            candidate.material_id.clone(),
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
                                    RichText::new(role)
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
        self.poll_catalog_task();
        let busy = self.catalog_task.is_some();
        if let Some(editor) = &mut self.editor
            && editor.show(context, !busy)
        {
            self.reload_catalog();
            self.status =
                "CATALOG SAVED — 素材の情報を保存しました。候補を再生成してください。".into();
        }
        let (_, status_color, status_label) = self.system_status();
        egui::TopBottomPanel::bottom("status").show(context, |ui| {
            ui.horizontal(|ui| {
                theme::badge(ui, status_label, status_color);
                ui.label(RichText::new(&self.status).size(11.0).color(theme::MUTED));
            });
        });
        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BACKGROUND).inner_margin(20))
            .show(context, |ui| {
                self.show_header(ui);
                ui.add_space(12.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(2, |columns| {
                        let controls = &mut columns[0];
                        self.show_catalog_controls(controls);
                        controls.add_space(8.0);
                        controls.add_enabled_ui(!busy, |ui| self.show_guide_controls(ui));
                        controls.add_space(10.0);
                        self.show_candidate_grid(controls);

                        let stage = &mut columns[1];
                        stage.add_enabled_ui(!busy, |ui| self.show_stage(ui));
                    });
                });
            });
    }
}

fn section_header(ui: &mut Ui, title: &str, signal: theme::Signal, detail: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(38.0);
        theme::caption(ui, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            theme::traffic_light(ui, signal).on_hover_text(detail);
        });
    });
}

fn guide_text_from_inputs(guide_inputs: &[GuideInput]) -> String {
    guide_inputs
        .iter()
        .filter(|guide_input| !guide_input.query.trim().is_empty())
        .map(|guide_input| format!("{}: {}", guide_input.role, guide_input.query.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn demo_material_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("demo")
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
                    egui::Label::new(RichText::new(&candidate.material_id).size(12.0).strong())
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
        ui.id().with(&candidate.material_id),
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
