mod state;

use std::sync::Arc;

use eframe::egui;
use std::sync::mpsc;

use crate::{
    gui::state::GUIState,
    transport::WebrtcTransport,
    video::{self, ScreenCapturer},
};

pub(super) struct GUI {
    frame_rx: Option<mpsc::Receiver<super::video::Frame>>,
    webrtc: Arc<WebrtcTransport>,
    webrtc_rt: Arc<tokio::runtime::Runtime>,
    state: GUIState,
}

impl GUI {
    pub fn new(webrtc: Arc<WebrtcTransport>, webrtc_rt: Arc<tokio::runtime::Runtime>) -> Self {
        GUI {
            frame_rx: None,
            state: GUIState::Idle,
            webrtc_rt,
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
                        GUIState::Idle => {
                            match video::XCapCapturer::new() {
                                Err(err) => {
                                    tracing::error!("Could start stream: {err}");
                                    return;
                                }
                                Ok(capturer) => match capturer.start_capturing() {
                                    Err(err) => {
                                        tracing::error!("Could start stream: {err}");
                                        return;
                                    }
                                    Ok(frame_rx) => self.frame_rx = Some(frame_rx),
                                },
                            };
                            let webrtc_clone = Arc::clone(&self.webrtc);
                            self.webrtc_rt.spawn(webrtc_clone.create_and_send_offers());
                        }
                        GUIState::Streaming => {
                            self.frame_rx = None;
                            self.state = GUIState::Idle
                        }
                        GUIState::Watching => {
                            tracing::error!("Cannot start stream because watching")
                        }
                    }
                }
            })
        });
    }
}
