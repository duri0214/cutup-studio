use cutup_studio::ui::app::StudioApp;
use eframe::egui;

fn main() -> eframe::Result<()> {
    eframe::run_native(
        "cutup-studio",
        native_options(),
        Box::new(|creation_context| Ok(Box::new(StudioApp::new(creation_context)))),
    )
}

fn native_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([1280.0, 640.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::native_options;
    use eframe::{Renderer, egui};

    #[test]
    fn native_options_use_wgpu_and_preserve_viewport_sizes() {
        let options = native_options();

        assert_eq!(options.renderer, Renderer::Wgpu);
        assert_eq!(options.viewport.inner_size, Some(egui::vec2(1280.0, 820.0)));
        assert_eq!(
            options.viewport.min_inner_size,
            Some(egui::vec2(1280.0, 640.0))
        );
    }
}
