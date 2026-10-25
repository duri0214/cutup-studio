use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Frame, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
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
    Open,
    Generate,
    Save,
    Reset,
}

pub fn install(context: &Context) {
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
    fonts
        .font_data
        .insert(font_name.clone(), Arc::new(FontData::from_owned(font_data)));
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

pub fn action_button(
    ui: &mut Ui,
    icon: ActionIcon,
    label: &str,
    primary: bool,
    size: Vec2,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    let visuals = ui.style().interact(&response);
    let fill = if primary && enabled {
        if response.is_pointer_button_down_on() {
            ACCENT.gamma_multiply(0.72)
        } else if response.hovered() {
            ACCENT.gamma_multiply(0.9)
        } else {
            ACCENT
        }
    } else {
        visuals.bg_fill
    };
    let text_color = if primary && enabled {
        BACKGROUND
    } else if enabled {
        TEXT
    } else {
        MUTED
    };
    let stroke = Stroke::new(
        1.0_f32,
        if primary && enabled {
            ACCENT
        } else {
            visuals.bg_stroke.color
        },
    );
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    ui.painter()
        .rect_stroke(rect, CornerRadius::same(6), stroke, StrokeKind::Inside);

    let icon_center = Pos2::new(rect.left() + 18.0, rect.center().y);
    draw_action_icon(ui, icon, icon_center, text_color);
    ui.painter().text(
        Pos2::new(rect.left() + 34.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        text_color,
    );
    response
}

fn draw_action_icon(ui: &Ui, icon: ActionIcon, center: Pos2, color: Color32) {
    let painter = ui.painter();
    let stroke = Stroke::new(1.5_f32, color);
    match icon {
        ActionIcon::Open => {
            let folder = Rect::from_min_size(
                Pos2::new(center.x - 8.0, center.y - 3.0),
                Vec2::new(16.0, 10.0),
            );
            painter.rect_stroke(folder, CornerRadius::same(2), stroke, StrokeKind::Inside);
            painter.line_segment(
                [
                    Pos2::new(center.x - 7.0, center.y - 3.0),
                    Pos2::new(center.x - 2.0, center.y - 6.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 2.0, center.y - 6.0),
                    Pos2::new(center.x + 2.0, center.y - 3.0),
                ],
                stroke,
            );
        }
        ActionIcon::Generate => {
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - 8.0),
                    Pos2::new(center.x, center.y + 8.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 8.0, center.y),
                    Pos2::new(center.x + 8.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 4.5, center.y - 4.5),
                    Pos2::new(center.x + 4.5, center.y + 4.5),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 4.5, center.y + 4.5),
                    Pos2::new(center.x + 4.5, center.y - 4.5),
                ],
                stroke,
            );
        }
        ActionIcon::Save => {
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - 8.0),
                    Pos2::new(center.x, center.y + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 4.0, center.y + 1.0),
                    Pos2::new(center.x, center.y + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 4.0, center.y + 1.0),
                    Pos2::new(center.x, center.y + 5.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 7.0, center.y + 8.0),
                    Pos2::new(center.x + 7.0, center.y + 8.0),
                ],
                stroke,
            );
        }
        ActionIcon::Reset => {
            painter.line_segment(
                [
                    Pos2::new(center.x + 7.0, center.y - 4.0),
                    Pos2::new(center.x - 6.0, center.y - 4.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.0, center.y - 4.0),
                    Pos2::new(center.x - 2.0, center.y - 8.0),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.0, center.y - 4.0),
                    Pos2::new(center.x - 2.0, center.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 7.0, center.y - 4.0),
                    Pos2::new(center.x + 7.0, center.y + 7.0),
                ],
                stroke,
            );
        }
    }
}
