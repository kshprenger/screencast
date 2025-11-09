mod event_loop;
mod state;

use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use eframe::egui;
use std::sync::mpsc;
use tokio::sync::Mutex;

use crate::{
    gui::state::GUIState,
    transport::WebrtcTransport,
    video::{self, Frame, ScreenCapturer},
};

pub(super) struct GUI {
    frame_rx: Option<mpsc::Receiver<super::video::Frame>>,
    webrtc: Arc<WebrtcTransport>,
    async_rt: Arc<tokio::runtime::Runtime>,
    state: Arc<Mutex<GUIState>>, // Faster with AtomicPtr???
}

impl GUI {
    pub fn new(webrtc: Arc<WebrtcTransport>, async_rt: Arc<tokio::runtime::Runtime>) -> Self {
        let shared_state = Arc::new(Mutex::new(GUIState::Idle));
        let share_state_clone = Arc::clone(&shared_state);
        let events = webrtc.subscribe();

        // Special event loop changing gui state based on webrtc network events
        async_rt.spawn(event_loop::handle_events(events, share_state_clone));

        GUI {
            frame_rx: None,
            state: shared_state,
            async_rt,
            webrtc,
        }
    }
}

impl eframe::App for GUI {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let curr_state = &mut *self.state.blocking_lock();

        match curr_state {
            GUIState::Idle | GUIState::Streaming => {
                match curr_state {
                    GUIState::Streaming => {
                        self.async_rt
                            .spawn(Arc::clone(&self.webrtc).send_frame(Frame {
                                width: 0,
                                height: 0,
                                data: vec![123, 123, 123, 123, 123, 123, 123, 123, 123, 123],
                            }));
                        *curr_state = GUIState::Idle
                    }
                    _ => {}
                }
                egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
                    ui.horizontal_centered(|ui| {
                        if ui.button("Toggle stream").clicked() {
                            match curr_state {
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
                                    let webrtc_clone_offers = Arc::clone(&self.webrtc);
                                    self.async_rt
                                        .spawn(webrtc_clone_offers.create_and_send_offers());
                                }
                                GUIState::Streaming => {
                                    self.frame_rx = None;
                                    *curr_state = GUIState::Idle
                                }
                                // Safety: outer match case with mutex
                                _ => unreachable!(),
                            }
                        }
                    })
                });
            }

            GUIState::Watching(_) => {
                egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
                    ui.button("HIII").clicked();
                });
            }
        }
    }
}
