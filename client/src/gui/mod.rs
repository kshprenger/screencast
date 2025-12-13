mod event_manager;
mod state;

use egui::{ColorImage, TextureHandle, TextureOptions, Vec2};
pub use event_manager::GUIEventManager;
pub use state::GUIState;

use std::sync::Arc;

use eframe::egui;
use tokio::sync::Mutex;

pub(super) struct GUI {
    event_manager: Arc<GUIEventManager>,
    state: Arc<Mutex<GUIState>>, // Faster with AtomicPtr rel/acq ???
    texture: Option<TextureHandle>,
}

impl GUI {
    pub fn new(state: Arc<Mutex<GUIState>>, event_manager: Arc<GUIEventManager>) -> Self {
        GUI {
            state,
            event_manager,
            texture: None,
        }
    }
}

impl eframe::App for GUI {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let curr_state = &mut *self.state.blocking_lock();

        match curr_state {
            GUIState::Watching(frame_rx) => {
                let frame = frame_rx.recv().unwrap();
                let width = frame.width as usize;
                let height = frame.height as usize;
                if width == 0 || height == 0 {
                    return;
                }

                let color_image = ColorImage::from_rgba_unmultiplied([width, height], &frame.data);

                match &mut self.texture {
                    Some(texture) => {
                        texture.set(color_image, TextureOptions::NEAREST);
                    }
                    None => {
                        self.texture = Some(ctx.load_texture(
                            "video_frame",
                            color_image,
                            TextureOptions::NEAREST,
                        ));
                    }
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(texture) = &self.texture {
                        let available_size = ui.available_size();
                        let (rect, _) =
                            ui.allocate_exact_size(available_size, egui::Sense::hover());

                        // Get the full available size
                        let painter = ui.painter();
                        painter.image(
                            texture.id(),
                            rect, // Target rectangle (full available space)
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, 0.0), // Top-left UV
                                egui::pos2(1.0, 1.0), // Bottom-right UV (FULL texture)
                            ),
                            egui::Color32::WHITE,
                        );
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Connecting to stream...");
                        });
                    }
                });

                ctx.request_repaint();
            }
            _ => {
                egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
                    ui.horizontal_centered(|ui| {
                        if ui.button("Toggle stream").clicked() {
                            match curr_state {
                                GUIState::Idle => self.event_manager.start_stream(),
                                GUIState::Streaming => self.event_manager.stop_stream(),
                                _ => unreachable!(),
                            }
                        }
                    })
                });
            }
        }
    }
}
