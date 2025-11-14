mod event_manager;
mod state;

use egui::{ColorImage, TextureHandle, TextureOptions, Vec2};
pub use event_manager::GUIEventManager;
pub use state::GUIState;

use std::sync::Arc;

use eframe::egui;
use std::sync::Mutex;

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
        let curr_state = &mut *self.state.lock().unwrap();

        match curr_state {
            GUIState::Watching(frame_rx) => {
                // Try to receive a frame without blocking
                if let Ok(frame) = frame_rx.try_recv() {
                    // Use the Frame struct fields
                    let width = frame.width as usize;
                    let height = frame.height as usize;
                    let bgra_data = &frame.data;

                    // Create or update texture
                    let color_image = ColorImage::from_rgba_unmultiplied(
                        [width, height],
                        bgra_data
                            .chunks_exact(4)
                            .flat_map(|bgra| [bgra[2], bgra[1], bgra[0], bgra[3]]) // B,G,R,A -> R,G,B,A
                            .collect::<Vec<_>>()
                            .as_slice(),
                    );

                    self.texture =
                        Some(ctx.load_texture("video_frame", color_image, TextureOptions::LINEAR));
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(texture) = &self.texture {
                        // Get available space
                        let available_size = ui.available_size();

                        // Calculate aspect ratio preserving size
                        let texture_size = texture.size_vec2();
                        let aspect_ratio = texture_size.x / texture_size.y;

                        let display_size = if available_size.x / available_size.y > aspect_ratio {
                            // Window is wider than video
                            Vec2::new(available_size.y * aspect_ratio, available_size.y)
                        } else {
                            // Window is taller than video
                            Vec2::new(available_size.x, available_size.x / aspect_ratio)
                        };

                        // Center the image
                        let (rect, _) = ui.allocate_exact_size(display_size, egui::Sense::hover());
                        ui.put(
                            rect,
                            egui::Image::new(texture).fit_to_exact_size(display_size),
                        );
                    } else {
                        ui.centered_and_justified(|ui| {
                            ui.label("Connecting to stream...");
                        });
                    }
                });

                // Request repaint for smooth video playback
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
