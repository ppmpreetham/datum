use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast;

const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1:8000";

struct AppState {
    tx: broadcast::Sender<String>,
}

static GYRO_SENDER: OnceLock<broadcast::Sender<String>> = OnceLock::new();

#[allow(dead_code)]
pub fn publish_drone_attitude(roll_rad: f32, pitch_rad: f32, yaw_rad: f32) {
    let Some(sender) = GYRO_SENDER.get() else {
        return;
    };
    let message = serde_json::json!({
        "alpha": yaw_rad.to_degrees(),
        "beta": roll_rad.to_degrees(),
        "gamma": -pitch_rad.to_degrees(),
        "source": "mavlink"
    })
    .to_string();
    let _ = sender.send(message);
}

pub fn start() -> Result<(), String> {
    let (tx, _rx) = broadcast::channel(100);
    let _ = GYRO_SENDER.set(tx.clone());
    let app_state = Arc::new(AppState { tx });

    tauri::async_runtime::spawn(async move {
        let app = Router::new()
            .route("/ws/gyro", get(ws_handler))
            .with_state(app_state);

        let listener = match tokio::net::TcpListener::bind(DEFAULT_BIND_ADDRESS).await {
            Ok(listener) => listener,
            Err(error) => {
                log::error!(target: "datum::websocket", "bind failed address={DEFAULT_BIND_ADDRESS} error={error}");
                return;
            }
        };
        log::info!(target: "datum::websocket", "listening url=ws://{DEFAULT_BIND_ADDRESS}/ws/gyro");
        if let Err(error) = axum::serve(listener, app).await {
            log::error!(target: "datum::websocket", "server stopped error={error}");
        }
    });
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> axum::response::Response {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();
    let tx = state.tx.clone();

    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            let _ = tx.send(text);
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_is_local_only_by_default() {
        assert_eq!(DEFAULT_BIND_ADDRESS, "127.0.0.1:8000");
    }
}
