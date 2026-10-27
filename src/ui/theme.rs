use std::sync::Arc;

use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontTweak, Frame,
    Pos2, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

pub const BACKGROUND: Color32 = Color32::from_rgb(13, 17, 23);
pub const PANEL: Color32 = Color32::from_rgb(23, 29, 38);
pub const BORDER: Color32 = Color32::from_rgb(47, 58, 72);
pub const MUTED: Color32 = Color32::from_rgb(143, 158, 179);
pub const TEXT: Color32 = Color32::from_rgb(229, 236, 245);
pub const ACCENT: Color32 = Color32::from_rgb(81, 224, 196);
pub const AMBER: Color32 = Color32::from_rgb(248, 193, 91);
pub const RED: Color32 = Color32::from_rgb(255, 100, 119);

#[derive(Clone, Copy)]
pub enum ActionIcon {
    New,
    Open,
    Generate,
    Save,
    Reset,
    Delete,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Error,
    Attention,
    Ready,
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
    style.visuals.selection.bg_fill = Color32::from_rgb(29, 78, 76);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 44, 56);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(34, 44, 56);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(44, 65, 74);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(44, 65, 74);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(35, 90, 84);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(35, 90, 84);
    for visuals in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        visuals.corner_radius = CornerRadius::same(6);
    }
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.interact_size.y = 30.0;
    context.set_style(style);
}

fn install_japanese_font(context: &Context) {
    let Some(windows_directory) = std::env::var_os("WINDIR") else {
        return;
    };
    let path = std::path::Path::new(&windows_directory)
        .join("Fonts")
        .join("YuGothM.ttc");
    let Ok(font_data) = std::fs::read(path) else {
        return;
    };

    let mut fonts = FontDefinitions::default();
    let font_name = "yu-gothic".to_owned();
    fonts.font_data.insert(
        font_name.clone(),
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
            .insert(0, font_name.clone());
    }
    context.set_fonts(fonts);
}

pub fn panel() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(10)
        .inner_margin(14)
}

pub fn caption(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(10.0).strong().color(MUTED));
}

pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    Frame::new()
        .fill(color.gamma_multiply(0.12))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.0).strong().color(color));
        });
}

pub fn traffic_light(ui: &mut Ui, active: Signal) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(18.0, 38.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(6), Color32::from_rgb(8, 11, 15));
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, BORDER),
        StrokeKind::Inside,
    );
    for (signal, color, offset) in [
        (Signal::Error, RED, 8.0),
        (Signal::Attention, AMBER, 19.0),
        (Signal::Ready, ACCENT, 30.0),
    ] {
        let color = if signal == active {
            color
        } else {
            color.gamma_multiply(0.18)
        };
        ui.painter()
            .circle_filled(Pos2::new(rect.center().x, rect.top() + offset), 3.5, color);
    }
    response
}

pub fn role_badge(ui: &mut Ui, text: &str, color: Color32) {
    ui.allocate_ui_with_layout(
        Vec2::new(48.0, 24.0),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| badge(ui, text, color),
    );
}

pub fn badge_button(ui: &mut Ui, text: &str, color: Color32) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).size(10.0).strong().color(color))
            .fill(color.gamma_multiply(0.12))
            .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.35)))
            .min_size(Vec2::new(0.0, 24.0)),
    )
}

pub fn icon_button(ui: &mut Ui, icon: ActionIcon, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(28.0, 24.0), Sense::click());
    let visuals = ui.style().interact(&response);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(5), visuals.bg_fill);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(5),
        visuals.bg_stroke,
        StrokeKind::Inside,
    );
    paint_action_icon(ui, icon, rect.center(), Vec2::splat(16.0), TEXT);
    response.on_hover_text(tooltip)
}

pub fn drag_handle(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(24.0, 24.0), Sense::drag());
    let color = if response.hovered() || response.dragged() {
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

pub fn action_button(
    ui: &mut Ui,
    icon: ActionIcon,
    label: &str,
    primary: bool,
    size: Vec2,
) -> Response {
    let enabled = ui.is_enabled();
    let text_color = if primary && enabled {
        BACKGROUND
    } else if enabled {
        TEXT
    } else {
        MUTED
    };
    let mut button = egui::Button::image_and_text(
        action_icon_image(icon),
        RichText::new(label).size(12.0).color(text_color),
    )
    .image_tint_follows_text_color(true)
    .min_size(size)
    .corner_radius(6);
    if primary && enabled {
        button = button.fill(ACCENT).stroke(Stroke::new(1.0_f32, ACCENT));
    }
    ui.add_sized(size, button)
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
            "../../assets/icons/lucide/folder-open.svg"
        )),
        ActionIcon::Generate => egui::Image::new(egui::include_image!(
            "../../assets/icons/lucide/wand-sparkles.svg"
        )),
        ActionIcon::Save => egui::Image::new(egui::include_image!(
            "../../assets/icons/lucide/download.svg"
        )),
        ActionIcon::Reset => egui::Image::new(egui::include_image!(
            "../../assets/icons/lucide/rotate-ccw.svg"
        )),
        ActionIcon::Delete => {
            egui::Image::new(egui::include_image!("../../assets/icons/lucide/trash.svg"))
        }
    }
}
