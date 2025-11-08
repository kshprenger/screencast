mod event_loop;
mod state;

use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

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
    async_rt: Arc<tokio::runtime::Runtime>,
    state: Arc<AtomicPtr<GUIState>>, // Should be fast, thats why we use raw pointers here
}

impl GUI {
    pub fn new(webrtc: Arc<WebrtcTransport>, async_rt: Arc<tokio::runtime::Runtime>) -> Self {
        let shared_state = Arc::new(AtomicPtr::new(Box::into_raw(Box::new(GUIState::Idle))));
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
        // Ordering: Receiving messages only from webrtc events handling event loop
        // Safety: Event loop & GUI contructor stores only valid values
        let curr_state = unsafe { &*self.state.load(Ordering::Acquire) };

        match curr_state {
            GUIState::Idle | GUIState::Streaming => {
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
                                }
                                // Safety: Stream button available while either idling or streaming
                                _ => unreachable!(),
                            }
                        }
                    })
                });
            }

            GUIState::Watching(_) => { /* Do not render stream button while watching */ }
        }
    }
}
