use cutup_studio::ui::app::StudioApp;
use eframe::egui;
fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([920.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "cutup-studio",
        options,
        Box::new(|creation_context| Ok(Box::new(StudioApp::new(creation_context)))),
    )
}
