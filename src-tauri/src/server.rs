use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::broadcast;

struct AppState {
    tx: broadcast::Sender<String>,
}

static BROADCAST_TX: OnceLock<broadcast::Sender<String>> = OnceLock::new();
static SERVER_STATUS: OnceLock<Arc<Mutex<StreamServerStatus>>> = OnceLock::new();

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamServerStatus {
    pub running: bool,
    pub bind_address: &'static str,
    pub websocket_path: &'static str,
    pub last_error: Option<String>,
}

#[derive(Serialize)]
struct DroneOrientationMessage {
    alpha: f64,
    beta: f64,
    gamma: f64,
    source: &'static str,
}

pub fn start() {
    let (tx, _rx) = broadcast::channel(100);
    if BROADCAST_TX.set(tx.clone()).is_err() {
        return;
    }
    let app_state = Arc::new(AppState { tx });
    let status = Arc::new(Mutex::new(StreamServerStatus {
        running: false,
        bind_address: "0.0.0.0:8000",
        websocket_path: "/ws/gyro",
        last_error: None,
    }));
    let _ = SERVER_STATUS.set(Arc::clone(&status));

    tauri::async_runtime::spawn(async move {
        let app = Router::new()
            .route("/ws/gyro", get(ws_handler))
            .with_state(app_state);

        let listener = match tokio::net::TcpListener::bind("0.0.0.0:8000").await {
            Ok(listener) => listener,
            Err(error) => {
                if let Ok(mut value) = status.lock() {
                    value.last_error = Some(format!("Failed to bind attitude WebSocket: {error}"));
                }
                eprintln!("Failed to bind attitude WebSocket: {error}");
                return;
            }
        };
        if let Ok(mut value) = status.lock() {
            value.running = true;
        }
        println!("WebSocket server running on ws://0.0.0.0:8000/ws/gyro");
        if let Err(error) = axum::serve(listener, app).await {
            if let Ok(mut value) = status.lock() {
                value.running = false;
                value.last_error = Some(format!("Attitude WebSocket stopped: {error}"));
            }
            eprintln!("Attitude WebSocket stopped: {error}");
        }
    });
}

#[tauri::command]
pub fn get_stream_server_status() -> StreamServerStatus {
    SERVER_STATUS
        .get()
        .and_then(|status| status.lock().ok().map(|value| value.clone()))
        .unwrap_or(StreamServerStatus {
            running: false,
            bind_address: "0.0.0.0:8000",
            websocket_path: "/ws/gyro",
            last_error: Some("The attitude WebSocket has not been started.".to_string()),
        })
}

/// Sends MAVLink ATTITUDE data through the existing gyro WebSocket in the
/// browser device-orientation shape expected by the 3D model.
pub fn publish_drone_attitude(roll_rad: f32, pitch_rad: f32, yaw_rad: f32) {
    let Some(tx) = BROADCAST_TX.get() else {
        return;
    };

    let message = DroneOrientationMessage {
        alpha: f64::from(yaw_rad).to_degrees(),
        beta: f64::from(roll_rad).to_degrees(),
        gamma: -f64::from(pitch_rad).to_degrees(),
        source: "mavlink",
    };

    if let Ok(message) = serde_json::to_string(&message) {
        let _ = tx.send(message);
    }
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
