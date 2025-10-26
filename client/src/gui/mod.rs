mod state;

use std::sync::Arc;

use eframe::egui;
use tokio::sync::mpsc;

use crate::{gui::state::GUIState, transport::WebrtcTransport};

pub(super) struct GUI {
    frame_rx: Option<mpsc::Receiver<super::video::Frame>>,
    webrtc: Arc<WebrtcTransport>,
    state: GUIState,
}

impl GUI {
    pub fn new(webrtc: Arc<WebrtcTransport>) -> Self {
        GUI {
            frame_rx: None,
            state: GUIState::Idle,
            webrtc,
        }
    }
}

impl eframe::App for GUI {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if ui.button("Toggle stream").clicked() {
                    match self.state {
                        GUIState::Idle => {}
                        GUIState::Streaming => {}
                        GUIState::Watching => {}
                    }
                }
            })
        });
    }
}
