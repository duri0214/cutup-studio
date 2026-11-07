use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, Ui, Vec2};

use crate::{
    domain::valueobject::catalog::ScanReport,
    domain::{
        service::create_plan,
        valueobject::{
            Candidate, Catalog, CompositionRecipe, CompositionRecipeSlot, Plan, RecipeSelection,
            SCHEMA_VERSION,
        },
    },
    infra::sqlite_catalog::{CatalogError, SqliteCatalog},
};

use super::catalog_editor::{CatalogEditor, PANEL_WIDTH, PanelEvent};
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
    selections: BTreeMap<String, Candidate>,
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
                    "CATALOG ERROR — フォルダ走査が中断されました。OPENで基準フォルダを開き直してください。".into();
                return;
            }
        };
        self.catalog_task = None;
        match result
            .and_then(|(store, path, report)| Ok((CatalogEditor::new(store)?, path, report)))
        {
            Ok((mut editor, path, report)) => {
                let total = editor.count();
                editor.open = total > 0;
                self.editor = Some(editor);
                self.catalog_path = Some(path);
                self.reload_catalog();
                self.status = if total == 0 {
                    "SCAN COMPLETE — 素材ファイルが見つかりません。素材をフォルダへ入れて OPEN で基準フォルダを開き直してください。".into()
                } else {
                    format!(
                        "SCAN COMPLETE — 追加 {} / 変更 {} / 移動 {} / 削除 {}。素材の情報を登録後、メイン画面の 02 / COMPOSITION GUIDE で候補を選べます。",
                        report.added, report.changed, report.moved, report.removed
                    )
                };
            }
            Err(error) => {
                // A failed rescan must not leave an old plan active.
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
            self.plan = None;
            self.selections.clear();
            self.active_slot = None;
            self.status = "OPEN で基準フォルダを開いてください。".into();
            return;
        };
        let plan = create_plan(catalog, &guide_text);
        if plan.slots.is_empty() {
            self.plan = None;
            self.selections.clear();
            self.active_slot = None;
            self.status = "02 / COMPOSITION GUIDE に語句またはタグを入力してください。".into();
            return;
        }
        let candidate_count: usize = plan.slots.iter().map(|slot| slot.candidates.len()).sum();
        self.selections
            .retain(|slot_id, _| plan.slots.iter().any(|slot| slot.slot.id == *slot_id));
        self.active_slot = Some(0);
        self.status = format!(
            "{}件の候補を表示しました — 02 / COMPOSITION GUIDE で素材を選んでください。",
            candidate_count
        );
        self.plan = Some(plan);
    }

    fn current_recipe(&self) -> Option<CompositionRecipe> {
        let plan = self.plan.as_ref()?;
        Some(CompositionRecipe {
            slots: plan
                .slots
                .iter()
                .map(|slot| CompositionRecipeSlot {
                    slot: slot.slot.clone(),
                    selection: self.selections.get(&slot.slot.id).map(|candidate| {
                        RecipeSelection {
                            material_id: candidate.material_id.clone(),
                            score: candidate.score,
                            reasons: candidate.reasons.clone(),
                        }
                    }),
                })
                .collect(),
        })
    }

    fn save_recipe(&mut self) {
        let Some(recipe) = self.current_recipe() else {
            self.status = "構成設定を保存するには、02で語句を入力してください。".into();
            return;
        };
        let Some(editor) = &mut self.editor else {
            self.status = "構成設定を保存するには、基準フォルダを開いてください。".into();
            return;
        };
        match editor.store.save_recipe(&recipe) {
            Ok(()) => {
                self.status =
                    "構成設定を保存しました。必要なときに03の「読み込み」で復元できます。".into();
            }
            Err(error) => self.status = format!("RECIPE ERROR — 構成設定を保存できません: {error}"),
        }
    }

    fn load_recipe(&mut self) {
        let result = self
            .editor
            .as_ref()
            .map(|editor| editor.store.load_recipe());
        let Some(result) = result else {
            self.status = "構成設定を読み込むには、基準フォルダを開いてください。".into();
            return;
        };
        match result {
            Ok(Some(recipe)) => {
                self.guide_inputs = recipe
                    .slots
                    .iter()
                    .map(|recipe_slot| GuideInput {
                        role: recipe_slot.slot.role.clone(),
                        query: recipe_slot.slot.query.clone(),
                    })
                    .collect();
                self.build_plan();
                self.selections.clear();
                let mut restored_count = 0;
                let mut unavailable_count = 0;
                if let Some(plan) = &self.plan {
                    for recipe_slot in recipe.slots {
                        let Some(selection) = recipe_slot.selection else {
                            continue;
                        };
                        let Some(current) = plan
                            .slots
                            .iter()
                            .find(|slot| slot.slot.order == recipe_slot.slot.order)
                            .and_then(|slot| {
                                slot.candidates.iter().find(|candidate| {
                                    candidate.material_id == selection.material_id
                                })
                            })
                        else {
                            unavailable_count += 1;
                            continue;
                        };
                        let mut candidate = current.clone();
                        candidate.score = selection.score;
                        candidate.reasons = selection.reasons;
                        self.selections
                            .insert(recipe_slot.slot.id.clone(), candidate);
                        restored_count += 1;
                    }
                }
                self.active_slot = Some(0);
                self.status = if unavailable_count == 0 {
                    format!(
                        "構成設定を読み込みました。{restored_count}件の選択を03で確認できます。"
                    )
                } else {
                    format!(
                        "構成設定を読み込みました。{restored_count}件を復元、{unavailable_count}件は現在の候補にありません。"
                    )
                };
            }
            Ok(None) => {
                self.status =
                    "保存済みの構成設定がありません。02で構成を作って保存してください。".into();
            }
            Err(error) => self.status = format!("RECIPE ERROR — 構成設定を読み込めません: {error}"),
        }
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

    fn system_status(&self) -> (theme::State, &'static str) {
        if self.status.starts_with("CATALOG ERROR") || self.status.starts_with("CATALOG EXISTS") {
            (theme::State::Error, "ERROR")
        } else if self.catalog_task.is_some() {
            (theme::State::Attention, "SCANNING")
        } else if self.catalog.is_none() {
            (theme::State::Attention, "NO CATALOG")
        } else if self.is_demo_catalog() {
            (theme::State::Ready, "DEMO")
        } else {
            (theme::State::Ready, "READY")
        }
    }

    fn guide_status(&self) -> (theme::State, &'static str) {
        if self.catalog.is_none() {
            (theme::State::Attention, "01 の素材カタログを開いてください")
        } else if let Some(plan) = &self.plan {
            let active_index = self.active_slot.unwrap_or_default();
            match plan.slots.get(active_index) {
                Some(slot) if slot.candidates.is_empty() => (
                    theme::State::Error,
                    "候補がありません。02 の語句・タグ・役割を確認してください",
                ),
                Some(_) => (theme::State::Ready, "候補を選択できます"),
                None => (
                    theme::State::Attention,
                    "02 / COMPOSITION GUIDE に語句またはタグを入力してください",
                ),
            }
        } else {
            (
                theme::State::Attention,
                "02 / COMPOSITION GUIDE に語句またはタグを入力してください",
            )
        }
    }

    fn show_header(&self, ui: &mut Ui) {
        let (signal, _) = self.system_status();
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            ui.label(theme::brand_primary("CUTUP"));
            ui.label(theme::brand_secondary("STUDIO"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.is_demo_catalog() && signal != theme::State::Error {
                    theme::ready_badge(ui, "DEMO");
                }
                ui.label(theme::caption_text(format!("SCHEMA V{SCHEMA_VERSION}")).monospace());
            });
        });
        ui.label(theme::body_muted(
            "基準フォルダ内の素材から、構成の指示書を作る。",
        ));
    }

    fn show_catalog_controls(&mut self, ui: &mut Ui) {
        let (signal, _) = self.system_status();
        theme::panel().show(ui, |ui| {
            section_header(ui, "01 / MATERIAL CATALOG", signal, &self.status);
            ui.label(theme::detail_muted(
                "基準フォルダには materials/ と material-catalog.sqlite が必要です。",
            ));
            ui.add_space(theme::SPACE_XS);
            ui.add_enabled_ui(self.catalog_task.is_none(), |ui| {
                ui.horizontal(|ui| {
                    if theme::action_button(
                        ui,
                        theme::ActionIcon::New,
                        "NEW",
                        theme::ButtonKind::Normal,
                        Vec2::new(106.0, 32.0),
                    )
                    .clicked()
                    {
                        self.create_catalog(ui.ctx());
                    }
                    if theme::action_button(
                        ui,
                        theme::ActionIcon::Open,
                        "OPEN",
                        theme::ButtonKind::Normal,
                        Vec2::new(106.0, 32.0),
                    )
                    .clicked()
                    {
                        self.open_catalog(ui.ctx());
                    }
                    if theme::toggle_button(
                        ui,
                        "DEMO",
                        self.is_demo_catalog(),
                        Vec2::new(106.0, 32.0),
                    )
                    .clicked()
                    {
                        if self.is_demo_catalog() {
                            self.clear_active_catalog();
                            self.catalog_task = None;
                            self.status =
                                "NO CATALOG — NEW または OPEN で基準フォルダを選んでください。"
                                    .into();
                        } else {
                            self.use_demo_catalog(ui.ctx());
                        }
                    }
                });
                if let Some(editor) = &mut self.editor {
                    let total = editor.count();
                    ui.label(theme::body(format!("素材ファイル: {total} 件")));
                }
                let can_open_editor = self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.count() > 0 && !editor.open);
                if ui
                    .add_enabled_ui(can_open_editor, |ui| {
                        theme::button(ui, "素材情報を編集 →", theme::ButtonKind::Primary)
                    })
                    .inner
                    .clicked()
                    && let Some(editor) = &mut self.editor
                {
                    editor.open = true;
                }
            });
            if let Some(catalog) = &self.catalog {
                ui.horizontal(|ui| {
                    ui.label(theme::caption_text(format!(
                        "情報を登録済みの素材: {} 件",
                        catalog.materials.len()
                    )));
                    if self.catalog_path.is_some() {
                        ui.add(
                            egui::Label::new(theme::caption_text(format!(
                                "基準フォルダ: {}",
                                catalog.material_root
                            )))
                            .truncate(),
                        );
                    }
                });
            }
        });
    }

    fn show_guide_controls(&mut self, ui: &mut Ui) {
        let available_roles = self.available_roles();
        let (signal, status) = self.guide_status();
        let mut role_to_add = None;
        let mut guide_changed = false;
        let mut reset_selections = false;
        theme::panel().show(ui, |ui| {
            section_header(ui, "02 / COMPOSITION GUIDE", signal, status);
            ui.label(theme::detail_muted(
                "役割を選び、探したい語句・タグを入力します。",
            ));
            ui.horizontal_wrapped(|ui| {
                theme::caption(ui, "ADD SLOT");
                for role in &available_roles {
                    if theme::role_badge_button(ui, role).clicked() {
                        role_to_add = Some(role.clone());
                    }
                }
            });
            ui.add_space(theme::SPACE_XS);
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
                    theme::role_label(ui, &guide_input.role);
                    let response =
                        theme::text_input(ui, &mut guide_input.query, "探したい語句・タグ", 330.0);
                    guide_changed |= response.changed();
                    if can_remove
                        && theme::icon_button(
                            ui,
                            theme::ActionIcon::Delete,
                            "このスロットを削除",
                            theme::ButtonKind::Danger,
                        )
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
                    theme::drag_target_indicator(ui, row.response.rect);
                }
            }
            if let Some(index) = remove_index {
                self.guide_inputs.remove(index);
                guide_changed = true;
                reset_selections = true;
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
                    guide_changed = true;
                    reset_selections = true;
                }
                self.dragged_guide_index = None;
            }
            if let Some(role) = role_to_add {
                self.guide_inputs.push(GuideInput {
                    role,
                    query: String::new(),
                });
            }
            if guide_changed {
                if reset_selections {
                    self.selections.clear();
                }
                self.build_plan();
            }
        });
    }

    fn show_candidate_pane(&mut self, ui: &mut Ui) {
        theme::panel().show(ui, |ui| {
            section_header(
                ui,
                "02 / CANDIDATES",
                theme::State::Ready,
                "02 / COMPOSITION GUIDE の候補",
            );
            self.show_candidates(ui);
        });
    }

    fn show_info_pane(&self, ui: &mut Ui) {
        theme::panel().show(ui, |ui| {
            theme::caption(ui, "INFO PANE");
            ui.add_space(theme::SPACE_SM);
            ui.label(theme::body_muted(
                "素材の登録と、02 / COMPOSITION GUIDE の候補を表示します。",
            ));
        });
    }

    fn show_candidates(&mut self, ui: &mut Ui) {
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
            ui.label(theme::detail("候補").strong());
            ui.centered_and_justified(|ui| {
                ui.label(theme::body_muted("語句を入力すると候補が表示されます"));
            });
            return;
        };
        let slot_id = slot.slot.id.clone();
        let candidates = slot.candidates.clone();
        let selected = self.selections.get(&slot_id).cloned();
        let mut next_active_index = active_index;
        ui.label(theme::detail("候補").strong());
        if slot_tabs.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for (index, label) in slot_tabs.iter().enumerate() {
                    if theme::tab(ui, label, index == active_index, 132.0).clicked() {
                        next_active_index = index;
                    }
                }
            });
            ui.add_space(theme::SPACE_SM);
        }
        ui.label(theme::detail_muted(if slot_tabs.len() == 1 {
            format!("役割: {} / 語句: {}", slot.slot.role, slot.slot.query)
        } else {
            format!("語句: {}", slot.slot.query)
        }));
        ui.add_space(theme::SPACE_XS);
        if candidates.is_empty() {
            ui.add_space(theme::SPACE_LG);
            theme::state_message(
                ui,
                theme::State::Error,
                "候補がありません。ガイドの語句・タグ・役割を確認してください。",
            );
        } else {
            let card_width = ((ui.available_width() - theme::SPACE_MD) / 2.0).max(180.0);
            let mut chosen = None;
            for row in candidates.chunks(2) {
                ui.horizontal_top(|ui| {
                    for candidate in row {
                        let is_selected = selected
                            .as_ref()
                            .is_some_and(|current| current.material_id == candidate.material_id);
                        let reasons = candidate.reasons.join(" / ");
                        if theme::candidate_card(
                            ui,
                            &candidate.material_id,
                            &candidate.source_path,
                            &reasons,
                            is_selected,
                            card_width,
                        )
                        .on_hover_text(if is_selected {
                            "クリックで選択を解除"
                        } else {
                            "クリックで選択"
                        })
                        .clicked()
                        {
                            chosen = Some(candidate.clone());
                        }
                    }
                });
                ui.add_space(theme::SPACE_XS);
            }
            if let Some(candidate) = chosen {
                if selected
                    .as_ref()
                    .is_some_and(|current| current.material_id == candidate.material_id)
                {
                    self.selections.remove(&slot_id);
                    self.status =
                        "候補の選択を解除しました — 02 / COMPOSITION GUIDE で素材を選んでください。"
                            .into();
                } else {
                    self.selections.insert(slot_id, candidate);
                    self.status =
                        "候補を選択しました — ほかの役割も選び、03 / COMPOSITION で全体を確認してください。"
                            .into();
                }
            }
        }
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
                        let candidate = self.selections.get(&slot.slot.id).cloned();
                        (slot.slot.order, slot.slot.role.clone(), candidate)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let selected_count = entries
            .iter()
            .filter(|(_, _, candidate)| candidate.is_some())
            .count();
        let mut save_requested = false;
        let mut load_requested = false;
        theme::panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(32.0);
                theme::caption(ui, "03 / COMPOSITION");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let can_load = self.editor.is_some();
                    let can_save = self.plan.is_some() && self.editor.is_some();
                    if ui
                        .add_enabled_ui(can_load, |ui| {
                            theme::button(ui, "読み込み", theme::ButtonKind::Normal)
                        })
                        .inner
                        .clicked()
                    {
                        load_requested = true;
                    }
                    if ui
                        .add_enabled_ui(can_save, |ui| {
                            theme::button(ui, "保存", theme::ButtonKind::Primary)
                        })
                        .inner
                        .clicked()
                    {
                        save_requested = true;
                    }
                    theme::selection_count_badge(ui, selected_count, entries.len());
                });
            });
            ui.label(theme::detail_muted(
                "02で選んだ素材の順番を確認します。保存した構成設定は必要なときに読み込めます。",
            ));
            ui.add_space(theme::SPACE_SM);
            if entries.is_empty() {
                ui.add_space(theme::SPACE_XL);
                ui.horizontal_centered(|ui| {
                    ui.label(theme::body_muted(
                        "02 / COMPOSITION GUIDE に語句を入力すると構成が表示されます",
                    ));
                });
                ui.add_space(theme::SPACE_XL);
            }
            for (order, role, candidate) in entries.iter().filter_map(|(order, role, candidate)| {
                candidate.as_ref().map(|candidate| (order, role, candidate))
            }) {
                theme::selected_summary_item(ui, *order, &candidate.source_path, role);
                ui.add_space(theme::SPACE_XXS);
            }
        });
        if save_requested {
            self.save_recipe();
        }
        if load_requested {
            self.load_recipe();
        }
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_catalog_task();
        let busy = self.catalog_task.is_some();
        let registering = self.editor.as_ref().is_some_and(|editor| editor.open);
        let mut panel_event = PanelEvent::None;
        let (status_state, status_label) = self.system_status();
        egui::TopBottomPanel::top("header")
            .frame(theme::content_frame())
            .show(context, |ui| self.show_header(ui));
        egui::TopBottomPanel::bottom("status").show(context, |ui| {
            ui.horizontal(|ui| {
                theme::state_badge(ui, status_label, status_state);
                ui.label(theme::status_text(&self.status));
            });
        });
        egui::SidePanel::right("information-pane")
            .exact_width(PANEL_WIDTH)
            .resizable(false)
            .frame(theme::content_frame())
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if registering {
                        if let Some(editor) = &mut self.editor {
                            panel_event = editor.show_panel(ui, !busy);
                        }
                    } else if self.plan.is_some() {
                        self.show_candidate_pane(ui);
                    } else {
                        self.show_info_pane(ui);
                    }
                });
            });
        match panel_event {
            PanelEvent::None => {}
            PanelEvent::Closed => {
                self.status =
                    "MATERIAL EDITOR CLOSED — 01 / MATERIAL CATALOG の「素材情報を編集 →」で右ペインを開けます。"
                        .into();
            }
            PanelEvent::Saved => {
                self.reload_catalog();
                self.status = if self.editor.as_ref().is_some_and(|editor| editor.open) {
                    "CATALOG SAVED — 続けて素材の情報を登録できます。".into()
                } else {
                    "CATALOG SAVED — 素材の情報を保存しました。02 / COMPOSITION GUIDE で候補を選んでください。".into()
                };
            }
        }
        egui::CentralPanel::default()
            .frame(theme::content_frame())
            .show(context, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.show_catalog_controls(ui);
                    ui.add_space(theme::SPACE_SM);
                    ui.add_enabled_ui(!busy, |ui| self.show_guide_controls(ui));
                    ui.add_space(theme::SPACE_SM);
                    ui.add_enabled_ui(!busy, |ui| self.show_stage(ui));
                });
            });
    }
}

fn section_header(ui: &mut Ui, title: &str, signal: theme::State, detail: &str) {
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
