use std::collections::HashMap;

use cutup_studio::ui::app::StudioApp;
use eframe::egui;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, StartCause, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::WindowId,
};

struct DpiStableApplication<'a> {
    app: eframe::EframeWinitApplication<'a>,
    inner_sizes: HashMap<WindowId, PhysicalSize<u32>>,
}

fn main() -> eframe::Result<()> {
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    let app = eframe::create_native(
        "cutup-studio",
        native_options(),
        Box::new(|creation_context| Ok(Box::new(StudioApp::new(creation_context)))),
        &event_loop,
    );
    let mut app = DpiStableApplication {
        app,
        inner_sizes: HashMap::new(),
    };

    event_loop.run_app(&mut app)?;
    Ok(())
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

impl ApplicationHandler<eframe::UserEvent> for DpiStableApplication<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.app.resumed(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        mut event: WindowEvent,
    ) {
        match &mut event {
            WindowEvent::Resized(size) => {
                self.inner_sizes.insert(window_id, *size);
            }
            WindowEvent::ScaleFactorChanged {
                inner_size_writer, ..
            } => {
                if let Some(size) = self.inner_sizes.get(&window_id).copied() {
                    let _ = inner_size_writer.request_inner_size(size);
                }
            }
            WindowEvent::Destroyed => {
                self.inner_sizes.remove(&window_id);
            }
            _ => {}
        }

        self.app.window_event(event_loop, window_id, event);
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.app.new_events(event_loop, cause);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: eframe::UserEvent) {
        self.app.user_event(event_loop, event);
    }

    fn device_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        device_id: DeviceId,
        event: DeviceEvent,
    ) {
        self.app.device_event(event_loop, device_id, event);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.app.about_to_wait(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.app.suspended(event_loop);
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        self.app.exiting(event_loop);
    }

    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        self.app.memory_warning(event_loop);
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
