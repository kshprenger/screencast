mod event_manager;
mod state;

pub use event_manager::GUIEventManager;
pub use state::GUIState;

use std::sync::Arc;

use eframe::egui;
use std::sync::Mutex;

pub(super) struct GUI {
    event_manager: Arc<GUIEventManager>,
    state: Arc<Mutex<GUIState>>, // Faster with AtomicPtr rel/acq ???
}

impl GUI {
    pub fn new(state: Arc<Mutex<GUIState>>, event_manager: Arc<GUIEventManager>) -> Self {
        GUI {
            state,
            event_manager,
        }
    }
}

impl eframe::App for GUI {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let curr_state = &mut *self.state.lock().unwrap();
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
