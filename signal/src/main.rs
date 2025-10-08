use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use std::net::SocketAddr;
use tokio::sync::mpsc;
use uuid::Uuid;

mod config;
mod peers;

use peers::PeerManager;

#[tokio::main]
async fn main() {
    config::setup_logging();
    let cors = config::setup_cors();

    let peer_manager = PeerManager::new();

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(peer_manager)
        .layer(cors);

    use clap::Parser;
    let args = config::Args::parse();

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    tracing::info!("Starting server on {}", addr);

    match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => {
            if let Err(err) = axum::serve(listener, app.into_make_service()).await {
                tracing::error!("Could not axum::serve {}", err);
            }
        }
        Err(err) => {
            tracing::error!("Could not bind address {}", err);
        }
    };
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(peer_manager): State<PeerManager>,
) -> impl IntoResponse {
    let id = Uuid::new_v4();
    tracing::info!("New peer connected: {}", id);
    ws.on_upgrade(move |socket| handle_socket(socket, peer_manager, id))
}

async fn handle_socket(socket: WebSocket, peer_manager: PeerManager, id: Uuid) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::channel::<String>(100);

    peer_manager.add_peer(id, tx.clone()).await;

    // Task to send messages from the mpsc channel to the WebSocket client
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Err(err) = sender.send(Message::Text(msg)).await {
                tracing::warn!("Done send_task for {id}: {err}");
                break;
            }
        }
    });

    let peer_manager_clone = peer_manager.clone();

    // Task to receive messages from the WebSocket client and broadcast them
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            peer_manager_clone.broadcast_message(id, text).await;
        }
        tracing::warn!("Done recv_task for {id}");
    });

    // Wait for either task to complete and then cancel abort another one
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    peer_manager.remove_peer(&id).await;
}
