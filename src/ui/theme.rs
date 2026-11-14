use std::{hash::Hash, sync::Arc};

use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    FontTweak, Frame, Pos2, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(13, 17, 23);
pub const PANEL: Color32 = Color32::from_rgb(23, 29, 38);
pub const CONTROL: Color32 = Color32::from_rgb(34, 44, 56);
pub const CONTROL_HOVERED: Color32 = Color32::from_rgb(44, 65, 74);
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(35, 90, 84);
pub const SELECTED_BACKGROUND: Color32 = Color32::from_rgb(24, 48, 49);
pub const SELECTED_SUMMARY_BACKGROUND: Color32 = Color32::from_rgb(18, 39, 43);
pub const SELECTION_BACKGROUND: Color32 = Color32::from_rgb(29, 78, 76);
pub const TRAFFIC_BACKGROUND: Color32 = Color32::from_rgb(8, 11, 15);
pub const BORDER: Color32 = Color32::from_rgb(47, 58, 72);
pub const MUTED: Color32 = Color32::from_rgb(143, 158, 179);
pub const INACTIVE: Color32 = Color32::from_rgb(64, 71, 80);
pub const DISABLED: Color32 = Color32::from_rgb(104, 117, 133);
pub const TEXT: Color32 = Color32::from_rgb(229, 236, 245);
pub const ACCENT: Color32 = Color32::from_rgb(81, 224, 196);
pub const AMBER: Color32 = Color32::from_rgb(248, 193, 91);
pub const RED: Color32 = Color32::from_rgb(255, 100, 119);

pub const SPACE_XXS: f32 = 3.0;
pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 18.0;
pub const SPACE_XL: f32 = 24.0;

pub const PANEL_PADDING: i8 = 14;
pub const CONTENT_PADDING: i8 = 20;
pub const BADGE_HORIZONTAL_PADDING: i8 = 7;
pub const BADGE_VERTICAL_PADDING: i8 = 3;
pub const LIST_PADDING: i8 = 4;
pub const INPUT_HORIZONTAL_PADDING: i8 = 8;
pub const INPUT_VERTICAL_PADDING: i8 = 4;
pub const SUMMARY_PADDING: i8 = 10;

pub const PANEL_RADIUS: u8 = 10;
pub const CONTROL_RADIUS: u8 = 6;
pub const SMALL_CONTROL_RADIUS: u8 = 5;
pub const CARD_RADIUS: u8 = 8;
pub const BADGE_RADIUS: u8 = 4;

pub const BODY_TEXT_SIZE: f32 = 12.0;
pub const DETAIL_TEXT_SIZE: f32 = 11.0;
pub const CAPTION_TEXT_SIZE: f32 = 10.0;
pub const BRAND_TEXT_SIZE: f32 = 25.0;
pub const BUTTON_TEXT_SIZE: f32 = 12.0;
pub const TAB_TEXT_SIZE: f32 = 12.0;
pub const INPUT_HEIGHT: f32 = 30.0;
pub const FORM_LABEL_WIDTH: f32 = 90.0;
pub const SUGGESTION_LIST_HEIGHT: f32 = 220.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionIcon {
    New,
    Open,
    Delete,
    Close,
    Ollama,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Normal,
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    Selected,
    Attention,
    Error,
    Disabled,
}

pub fn install(context: &Context) {
    egui_extras::install_image_loaders(context);
    install_japanese_font(context);
    context.set_theme(egui::ThemePreference::Dark);
    let mut style = (*context.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = PANEL;
    style.visuals.selection.bg_fill = SELECTION_BACKGROUND;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.inactive.bg_fill = CONTROL;
    style.visuals.widgets.inactive.weak_bg_fill = CONTROL;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.hovered.bg_fill = CONTROL_HOVERED;
    style.visuals.widgets.hovered.weak_bg_fill = CONTROL_HOVERED;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.bg_fill = CONTROL_ACTIVE;
    style.visuals.widgets.active.weak_bg_fill = CONTROL_ACTIVE;
    for visuals in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        visuals.corner_radius = CornerRadius::same(CONTROL_RADIUS);
    }
    style.spacing.item_spacing = Vec2::new(10.0, SPACE_SM);
    style.spacing.button_padding = Vec2::new(SPACE_MD, 7.0);
    style.spacing.interact_size.y = INPUT_HEIGHT;
    context.set_style(style);
}

fn install_japanese_font(context: &Context) {
    let Some(windows_directory) = std::env::var_os("WINDIR") else {
        return;
    };
    let mut fonts = FontDefinitions::default();
    for (name, file) in [
        ("malgun-gothic", "malgun.ttf"),
        ("yu-gothic", "YuGothM.ttc"),
    ] {
        let path = std::path::Path::new(&windows_directory)
            .join("Fonts")
            .join(file);
        if let Ok(font_data) = std::fs::read(path) {
            fonts.font_data.insert(
                name.to_owned(),
                Arc::new(FontData::from_owned(font_data).tweak(FontTweak {
                    y_offset: 2.0,
                    ..Default::default()
                })),
            );
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .insert(0, name.to_owned());
            }
        }
    }
    context.set_fonts(fonts);
}

pub fn state_color(state: State) -> Color32 {
    match state {
        State::Ready | State::Selected => ACCENT,
        State::Attention => AMBER,
        State::Error => RED,
        State::Disabled => DISABLED,
    }
}

pub fn brand_primary(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .size(BRAND_TEXT_SIZE)
        .strong()
        .color(ACCENT)
}

pub fn brand_secondary(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(BRAND_TEXT_SIZE).strong()
}

pub fn body(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(BODY_TEXT_SIZE)
}

pub fn body_muted(text: impl Into<String>) -> RichText {
    body(text).color(MUTED)
}

pub fn detail(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).size(DETAIL_TEXT_SIZE)
}

pub fn form_label(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(FORM_LABEL_WIDTH, INPUT_HEIGHT),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(FORM_LABEL_WIDTH);
            ui.label(detail(text));
        },
    );
}

pub fn detail_muted(text: impl Into<String>) -> RichText {
    detail(text).color(MUTED)
}

pub fn caption_text(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .size(CAPTION_TEXT_SIZE)
        .strong()
        .color(MUTED)
}

pub fn section_title(text: impl Into<String>) -> RichText {
    body(text).strong().color(TEXT)
}

pub fn item_title(text: impl Into<String>) -> RichText {
    body(text).strong()
}

pub fn status_text(text: impl Into<String>) -> RichText {
    detail_muted(text)
}

pub fn input_hint(text: impl Into<String>) -> RichText {
    body_muted(text)
}

pub fn panel() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(PANEL_RADIUS)
        .inner_margin(PANEL_PADDING)
}

pub fn section_panel(selected: bool) -> Frame {
    panel().stroke(Stroke::new(1.0_f32, if selected { ACCENT } else { BORDER }))
}

pub fn content_frame() -> Frame {
    Frame::new().fill(BACKGROUND).inner_margin(CONTENT_PADDING)
}

pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(caption_text(text));
}

pub fn selected_badge(ui: &mut Ui, text: &str) {
    state_badge(ui, text, State::Selected);
}

pub fn ready_badge(ui: &mut Ui, text: &str) {
    state_badge(ui, text, State::Ready);
}

pub fn local_label(ui: &mut Ui) {
    ui.label(detail_muted("ローカル"));
}

pub fn state_badge(ui: &mut Ui, text: &str, state: State) {
    badge_with_color(ui, text, state_color(state));
}

fn badge_with_color(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(BADGE_RADIUS)
        .inner_margin(egui::Margin::symmetric(
            BADGE_HORIZONTAL_PADDING,
            BADGE_VERTICAL_PADDING,
        ))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .size(CAPTION_TEXT_SIZE)
                    .strong()
                    .color(color),
            );
        });
}

pub fn selection_count(ui: &mut Ui, selected: usize, total: usize) {
    ui.label(detail_muted(format!("選択 {selected} / {total}")));
}

pub fn state_message(ui: &mut Ui, state: State, text: &str) {
    ui.label(detail(text).color(state_color(state)));
}

pub fn traffic_light(ui: &mut Ui, active: State) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(18.0, 38.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), TRAFFIC_BACKGROUND);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(CONTROL_RADIUS),
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );
    for (state, offset) in [
        (State::Error, 8.0),
        (State::Attention, 19.0),
        (State::Ready, 30.0),
    ] {
        let color = if state == active {
            state_color(state)
        } else {
            state_color(state).gamma_multiply(0.18)
        };
        ui.painter()
            .circle_filled(Pos2::new(rect.center().x, rect.top() + offset), 3.5, color);
    }
    response
}

pub fn role_label(ui: &mut Ui, text: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(48.0, 24.0),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| {
            ui.label(detail(text));
        },
    );
}

pub fn role_badge_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .size(CAPTION_TEXT_SIZE)
                .strong()
                .color(ACCENT),
        )
        .fill(ACCENT.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0_f32, ACCENT.gamma_multiply(0.35)))
        .min_size(Vec2::new(0.0, 24.0)),
    )
}

pub fn tab(ui: &mut Ui, label: &str, selected: bool, width: f32) -> Response {
    let enabled = ui.is_enabled();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, INPUT_HEIGHT), sense);
    let text_color = if !enabled {
        state_color(State::Disabled)
    } else if selected {
        state_color(State::Selected)
    } else {
        MUTED
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(TAB_TEXT_SIZE),
        text_color,
    );
    if selected && enabled {
        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + SPACE_SM, rect.bottom() - 1.0),
                Pos2::new(rect.right() - SPACE_SM, rect.bottom() - 1.0),
            ],
            Stroke::new(2.0_f32, state_color(State::Selected)),
        );
    }
    response
}

pub fn text_input(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .desired_width(width)
            .min_size(Vec2::new(width, INPUT_HEIGHT))
            .margin(egui::Margin::symmetric(
                INPUT_HORIZONTAL_PADDING,
                INPUT_VERTICAL_PADDING,
            ))
            .hint_text(input_hint(hint)),
    )
}

pub fn suggestion_list(
    ui: &mut Ui,
    id_salt: impl Hash,
    max_height: f32,
    contents: impl FnOnce(&mut Ui),
) {
    Frame::new()
        .fill(BACKGROUND)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CONTROL_RADIUS)
        .inner_margin(LIST_PADDING)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id_salt)
                .max_height(max_height)
                .show(ui, |ui| contents(ui));
        });
}

pub fn suggestion_item(ui: &mut Ui, text: &str, state: Option<(&str, State)>) -> Response {
    ui.horizontal(|ui| {
        let response = ui.selectable_label(false, detail(text));
        if let Some((label, state)) = state {
            state_badge(ui, label, state);
        }
        response
    })
    .inner
}

pub fn candidate_card(
    ui: &mut Ui,
    id_salt: impl Hash,
    title: &str,
    detail: &str,
    selected: bool,
    width: f32,
) -> Response {
    let response = Frame::new()
        .fill(if selected { SELECTED_BACKGROUND } else { PANEL })
        .stroke(Stroke::new(1.0_f32, if selected { ACCENT } else { BORDER }))
        .corner_radius(CARD_RADIUS)
        .inner_margin(PANEL_PADDING)
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(width - 24.0);
                if selected {
                    selected_badge(ui, "選択中");
                }
                ui.add(egui::Label::new(item_title(title)).truncate());
                ui.add(
                    egui::Label::new(
                        RichText::new(detail)
                            .size(CAPTION_TEXT_SIZE)
                            .color(if selected { ACCENT } else { MUTED }),
                    )
                    .truncate(),
                );
            });
        })
        .response;
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(response.rect, ui.id().with(id_salt), sense);
    if selected || response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            CARD_RADIUS,
            Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, ACCENT),
            StrokeKind::Inside,
        );
    }
    response
}

pub fn selected_summary_item(ui: &mut Ui, order: usize, title: &str, detail: &str) {
    Frame::new()
        .fill(SELECTED_SUMMARY_BACKGROUND)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CONTROL_RADIUS)
        .inner_margin(SUMMARY_PADDING)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                selected_badge(ui, &format!("{order:02}"));
                ui.vertical(|ui| {
                    ui.label(item_title(title));
                    ui.label(detail_muted(detail));
                });
            });
        });
}

pub fn icon_button(ui: &mut Ui, icon: ActionIcon, tooltip: &str, kind: ButtonKind) -> Response {
    let sense = if ui.is_enabled() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(28.0, 24.0), sense);
    let visuals = ui.style().interact(&response);
    let icon_color = if !ui.is_enabled() {
        state_color(State::Disabled)
    } else if kind == ButtonKind::Danger {
        state_color(State::Error)
    } else {
        TEXT
    };
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(SMALL_CONTROL_RADIUS),
        visuals.bg_fill,
    );
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(SMALL_CONTROL_RADIUS),
        visuals.bg_stroke,
        StrokeKind::Inside,
    );
    paint_action_icon(ui, icon, rect.center(), Vec2::splat(16.0), icon_color);
    response
        .on_hover_text(tooltip)
        .on_disabled_hover_text(tooltip)
}

pub fn drag_handle(ui: &mut Ui) -> Response {
    let enabled = ui.is_enabled();
    let sense = if enabled {
        Sense::drag()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(24.0, 24.0), sense);
    let color = if !enabled {
        DISABLED
    } else if response.hovered() || response.dragged() {
        ACCENT
    } else {
        MUTED
    };
    egui::Image::new(egui::include_image!(
        "../../assets/icons/lucide/grip-vertical.svg"
    ))
    .fit_to_exact_size(Vec2::splat(16.0))
    .tint(color)
    .paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
    );
    response.on_hover_text("ドラッグして順番を入れ替え")
}

pub fn drag_target_indicator(ui: &Ui, rect: egui::Rect) {
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(2.0_f32, ACCENT),
    );
}

pub fn action_button(
    ui: &mut Ui,
    icon: ActionIcon,
    label: &str,
    kind: ButtonKind,
    size: Vec2,
) -> Response {
    let enabled = ui.is_enabled();
    let text_color = button_text_color(kind, enabled);
    let mut button = egui::Button::image_and_text(
        action_icon_image(icon),
        RichText::new(label)
            .size(BUTTON_TEXT_SIZE)
            .color(text_color),
    )
    .image_tint_follows_text_color(true)
    .min_size(size)
    .corner_radius(CONTROL_RADIUS);
    if enabled {
        button = match kind {
            ButtonKind::Primary => button.fill(ACCENT).stroke(Stroke::new(1.0_f32, ACCENT)),
            ButtonKind::Normal => button,
            ButtonKind::Danger => button
                .fill(RED.gamma_multiply(0.12))
                .stroke(Stroke::new(1.0_f32, RED.gamma_multiply(0.5))),
        };
    }
    ui.add_sized(size, button)
}

pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind) -> Response {
    let enabled = ui.is_enabled();
    let mut button = egui::Button::new(
        RichText::new(label)
            .size(BUTTON_TEXT_SIZE)
            .color(button_text_color(kind, enabled)),
    )
    .corner_radius(CONTROL_RADIUS);
    if enabled {
        button = match kind {
            ButtonKind::Primary => button.fill(ACCENT).stroke(Stroke::new(1.0_f32, ACCENT)),
            ButtonKind::Normal => button,
            ButtonKind::Danger => button
                .fill(RED.gamma_multiply(0.12))
                .stroke(Stroke::new(1.0_f32, RED.gamma_multiply(0.5))),
        };
    }
    ui.add(button)
}

fn button_text_color(kind: ButtonKind, enabled: bool) -> Color32 {
    if !enabled {
        state_color(State::Disabled)
    } else {
        match kind {
            ButtonKind::Primary => BACKGROUND,
            ButtonKind::Normal => TEXT,
            ButtonKind::Danger => RED,
        }
    }
}

pub fn toggle_button(ui: &mut Ui, label: &str, on: bool, size: Vec2) -> Response {
    let enabled = ui.is_enabled();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let text_color = if !enabled {
        state_color(State::Disabled)
    } else if on {
        state_color(State::Selected)
    } else {
        MUTED
    };
    ui.painter().text(
        Pos2::new(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(BUTTON_TEXT_SIZE),
        text_color,
    );
    let track = egui::Rect::from_center_size(
        Pos2::new(rect.right() - 24.0, rect.center().y),
        Vec2::new(28.0, 16.0),
    );
    ui.painter().rect_filled(
        track,
        CornerRadius::same(8),
        if !enabled {
            state_color(State::Disabled)
        } else if on {
            state_color(State::Selected)
        } else {
            INACTIVE
        },
    );
    let knob_x = if on {
        track.right() - 8.0
    } else {
        track.left() + 8.0
    };
    ui.painter().circle_filled(
        Pos2::new(knob_x, track.center().y),
        6.0,
        if !enabled {
            state_color(State::Disabled)
        } else if on {
            BACKGROUND
        } else {
            TEXT
        },
    );
    response.on_hover_text(if on {
        "DEMO は有効です"
    } else {
        "DEMO を開く"
    })
}

fn paint_action_icon(ui: &Ui, icon: ActionIcon, center: Pos2, size: Vec2, color: Color32) {
    action_icon_image(icon)
        .fit_to_exact_size(size)
        .tint(color)
        .paint_at(ui, egui::Rect::from_center_size(center, size));
}

fn action_icon_image(icon: ActionIcon) -> egui::Image<'static> {
    match icon {
        ActionIcon::New => egui::Image::new(egui::include_image!(
            "../../assets/icons/lucide/folder-plus.svg"
        )),
        ActionIcon::Open => egui::Image::new(egui::include_image!(
            "../../assets/icons/lucide/file-open.svg"
        )),
        ActionIcon::Delete => {
            egui::Image::new(egui::include_image!("../../assets/icons/lucide/trash.svg"))
        }
        ActionIcon::Close => {
            egui::Image::new(egui::include_image!("../../assets/icons/lucide/x.svg"))
        }
        ActionIcon::Ollama => egui::Image::new(egui::include_image!(
            "../../assets/icons/ollama/ollama-logo.svg"
        )),
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;

    #[test]
    fn form_labels_keep_following_controls_aligned_for_different_lengths() {
        let context = egui::Context::default();
        let mut positions = Vec::new();
        let _ = context.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for label in ["A", "Longer label"] {
                    ui.horizontal(|ui| {
                        super::form_label(ui, label);
                        positions.push(ui.cursor().left());
                    });
                }
            });
        });
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0], positions[1]);
    }

    #[test]
    fn ollama_logo_rasterizes_in_a_color_that_accepts_ui_tint() {
        let logo = egui_extras::image::load_svg_bytes(
            include_bytes!("../../assets/icons/ollama/ollama-logo.svg"),
            &Default::default(),
        )
        .unwrap();
        assert!(logo.pixels.iter().any(|pixel| {
            pixel.a() >= 200 && pixel.r() >= 200 && pixel.g() >= 200 && pixel.b() >= 200
        }));
    }
}
