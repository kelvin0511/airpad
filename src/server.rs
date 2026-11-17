use crate::certs::GeneratedTlsBundle;
use crate::config::{generate_token, Config};
use crate::input::{create_default_backend, InputProcessor};
use crate::protocol::{encode_pong, parse_client_packet, ClientPacket};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{header, HeaderMap, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{watch, Notify};
use tracing::{debug, info, warn};

#[derive(Embed)]
#[folder = "web/"]
struct WebAssets;

#[derive(Debug, Clone)]
pub struct ConnectedClientInfo {
    pub addr: String,
    pub connected_at: Instant,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StatusSnapshot {
    pub paused: bool,
}

pub struct AppState {
    pub config: RwLock<Config>,
    pub paused: AtomicBool,
    pub active_session_id: AtomicU64,
    pub session_notify: Notify,
    pub connected_client: RwLock<Option<ConnectedClientInfo>>,
    pub status_tx: watch::Sender<StatusSnapshot>,
    pub input: Mutex<InputProcessor>,
    pub ca_der: Vec<u8>,
    pub mobileconfig: Vec<u8>,
}

impl AppState {
    pub fn new(config: Config, ca_der: Vec<u8>, mobileconfig: Vec<u8>) -> Arc<Self> {
        let sens = config.sensitivity;
        let scroll = config.scroll_speed;
        let (status_tx, _) = watch::channel(StatusSnapshot { paused: false });
        Arc::new(Self {
            config: RwLock::new(config),
            paused: AtomicBool::new(false),
            active_session_id: AtomicU64::new(0),
            session_notify: Notify::new(),
            connected_client: RwLock::new(None),
            status_tx,
            input: Mutex::new(InputProcessor::new(create_default_backend(), sens, scroll)),
            ca_der,
            mobileconfig,
        })
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
        if paused {
            if let Ok(mut inp) = self.input.lock() {
                inp.release_all();
            }
        }
        let _ = self.status_tx.send(StatusSnapshot { paused });
    }

    pub fn disconnect_active(&self) {
        self.active_session_id.fetch_add(1, Ordering::SeqCst);
        self.session_notify.notify_waiters();
        if let Ok(mut guard) = self.connected_client.write() {
            *guard = None;
        }
        if let Ok(mut inp) = self.input.lock() {
            inp.release_all();
        }
    }

    pub fn rotate_token(&self) -> String {
        let new_token = generate_token();
        if let Ok(mut cfg) = self.config.write() {
            cfg.pairing_token = new_token.clone();
            let _ = cfg.save();
        }
        self.disconnect_active();
        new_token
    }

    pub fn set_sensitivity(&self, sensitivity: f32) {
        let clamped = sensitivity.clamp(0.2, 4.0);
        if let Ok(mut inp) = self.input.lock() {
            inp.sensitivity = clamped;
        }
        if let Ok(mut cfg) = self.config.write() {
            cfg.sensitivity = clamped;
            let _ = cfg.save();
        }
    }

    pub fn set_scroll_speed(&self, scroll_speed: f32) {
        let clamped = scroll_speed.clamp(0.2, 4.0);
        if let Ok(mut inp) = self.input.lock() {
            inp.scroll_speed = clamped;
        }
        if let Ok(mut cfg) = self.config.write() {
            cfg.scroll_speed = clamped;
            let _ = cfg.save();
        }
    }

    pub fn set_preferred_ip(&self, ip: String) {
        if let Ok(mut cfg) = self.config.write() {
            cfg.preferred_ip = Some(ip);
            let _ = cfg.save();
        }
    }

    pub fn connect_url(&self, ip: &str) -> String {
        let cfg = self.config.read().unwrap();
        format!("https://{}:{}/?token={}", ip, cfg.https_port, cfg.pairing_token)
    }

    pub fn setup_url(&self, ip: &str) -> String {
        let cfg = self.config.read().unwrap();
        format!(
            "http://{}:{}/?token={}",
            ip, cfg.http_onboarding_port, cfg.pairing_token
        )
    }
}

pub fn build_https_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health_handler))
        .fallback(static_asset_handler)
        .with_state(state)
}

pub fn build_http_onboarding_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(onboarding_page_handler))
        .route("/ca.mobileconfig", get(ca_mobileconfig_handler))
        .route("/ca.cer", get(ca_cer_handler))
        .with_state(state)
}

pub async fn run_servers(
    state: Arc<AppState>,
    tls_bundle: GeneratedTlsBundle,
    https_port: u16,
    http_port: u16,
) -> Result<(), String> {
    let https_addr = SocketAddr::from(([0, 0, 0, 0], https_port));
    let http_addr = SocketAddr::from(([0, 0, 0, 0], http_port));

    let https_app = build_https_router(state.clone());
    let http_app = build_http_onboarding_router(state.clone());

    let https_handle = axum_server::bind_rustls(https_addr, tls_bundle.rustls_config)
        .serve(https_app.into_make_service_with_connect_info::<SocketAddr>());

    let http_listener = tokio::net::TcpListener::bind(http_addr)
        .await
        .map_err(|e| format!("failed to bind HTTP onboarding port {http_port}: {e}"))?;

    let http_handle = axum::serve(
        http_listener,
        http_app.into_make_service_with_connect_info::<SocketAddr>(),
    );

    info!(
        "AirPad servers listening on HTTPS :{} and HTTP setup :{}",
        https_port, http_port
    );

    tokio::select! {
        res = https_handle => res.map_err(|e| format!("HTTPS server error: {e}")),
        res = http_handle => res.map_err(|e| format!("HTTP onboarding server error: {e}")),
    }
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true, "app": "airpad" }))
}

async fn static_asset_handler(uri: Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');
    let path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    match WebAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                content.data,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Query(query): Query<TokenQuery>,
    State(state): State<Arc<AppState>>,
) -> Response {
    let provided = query.token.unwrap_or_default();
    let expected = state.config.read().unwrap().pairing_token.clone();
    let token_valid = !provided.is_empty() && provided == expected;

    ws.on_upgrade(move |socket| handle_ws_connection(socket, addr, state, token_valid))
}

async fn handle_ws_connection(
    mut socket: WebSocket,
    addr: SocketAddr,
    state: Arc<AppState>,
    token_valid: bool,
) {
    if !token_valid {
        warn!("Rejected WebSocket from {} due to invalid pairing token", addr);
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code: 4001,
                reason: Cow::Borrowed("Invalid pairing token"),
            })))
            .await;
        return;
    }

    let my_session_id = state.active_session_id.fetch_add(1, Ordering::SeqCst) + 1;
    state.session_notify.notify_waiters();

    if let Ok(mut guard) = state.connected_client.write() {
        *guard = Some(ConnectedClientInfo {
            addr: addr.ip().to_string(),
            connected_at: Instant::now(),
        });
    }
    info!("Phone controller connected: {} (session {})", addr, my_session_id);

    let mut status_rx = state.status_tx.subscribe();
    let initial = status_rx.borrow().clone();
    if let Ok(json) = serde_json::to_string(&initial) {
        if socket.send(Message::Text(json)).await.is_err() {
            cleanup_session(&state, my_session_id);
            return;
        }
    }

    loop {
        tokio::select! {
            _ = state.session_notify.notified() => {
                if state.active_session_id.load(Ordering::SeqCst) != my_session_id {
                    let _ = socket.send(Message::Close(Some(CloseFrame {
                        code: 4000,
                        reason: Cow::Borrowed("Replaced by newer controller"),
                    }))).await;
                    break;
                }
            }
            changed = status_rx.changed() => {
                if changed.is_ok() {
                    let snap = status_rx.borrow().clone();
                    if let Ok(json) = serde_json::to_string(&snap) {
                        if socket.send(Message::Text(json)).await.is_err() {
                            break;
                        }
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Binary(bytes))) => {
                        match parse_client_packet(&bytes) {
                            Ok(ClientPacket::Ping { client_ts_ms }) => {
                                let pong = encode_pong(client_ts_ms);
                                if socket.send(Message::Binary(pong.to_vec())).await.is_err() {
                                    break;
                                }
                            }
                            Ok(pkt) => {
                                if !state.is_paused() {
                                    if let Ok(mut inp) = state.input.lock() {
                                        inp.process(pkt);
                                    }
                                }
                            }
                            Err(err) => {
                                debug!("Ignoring malformed packet from {}: {}", addr, err);
                            }
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    cleanup_session(&state, my_session_id);
    info!("Phone controller disconnected: {} (session {})", addr, my_session_id);
}

fn cleanup_session(state: &Arc<AppState>, session_id: u64) {
    if state.active_session_id.load(Ordering::SeqCst) == session_id {
        if let Ok(mut guard) = state.connected_client.write() {
            *guard = None;
        }
        if let Ok(mut inp) = state.input.lock() {
            inp.release_all();
        }
    }
}

async fn ca_mobileconfig_handler(State(state): State<Arc<AppState>>) -> Response {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/x-apple-aspen-config; charset=utf-8",
            ),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"airpad-ca.mobileconfig\"",
            ),
        ],
        state.mobileconfig.clone(),
    )
        .into_response()
}

async fn ca_cer_handler(State(state): State<Arc<AppState>>) -> Response {
    (
        [
            (header::CONTENT_TYPE, "application/pkix-cert"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"airpad-ca.cer\"",
            ),
        ],
        state.ca_der.clone(),
    )
        .into_response()
}

async fn onboarding_page_handler(
    headers: HeaderMap,
    Query(query): Query<TokenQuery>,
    State(state): State<Arc<AppState>>,
) -> Html<String> {
    let cfg = state.config.read().unwrap().clone();
    let host_raw = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let host_only = host_raw.split(':').next().unwrap_or("localhost");
    let token = query.token.unwrap_or_else(|| cfg.pairing_token.clone());
    let https_url = format!("https://{}:{}/?token={}", host_only, cfg.https_port, token);

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0, viewport-fit=cover" />
  <title>AirPad iPhone Setup</title>
  <style>
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0; padding: 20px;
      background: #0b0f17; color: #eef2f8;
      font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
      line-height: 1.5;
    }}
    .card {{
      max-width: 440px; margin: 16px auto;
      background: #131a26; border: 1px solid rgba(255,255,255,0.1);
      border-radius: 18px; padding: 20px;
    }}
    h1 {{ margin: 0 0 8px; font-size: 22px; }}
    p.sub {{ margin: 0 0 18px; color: #8b98ae; font-size: 14px; }}
    .step {{
      background: #1b2434; border-radius: 14px; padding: 14px; margin-bottom: 12px;
    }}
    .step-title {{ font-weight: 700; font-size: 15px; margin-bottom: 4px; }}
    .step-desc {{ font-size: 13px; color: #b5c0d4; }}
    .btn {{
      display: block; width: 100%; text-align: center; text-decoration: none;
      padding: 13px 16px; border-radius: 12px; font-weight: 700; font-size: 15px;
      margin-top: 10px;
    }}
    .btn-primary {{ background: #3b82f6; color: #fff; }}
    .btn-secondary {{ background: #26334a; color: #dbeafe; }}
    .note {{ font-size: 12px; color: #8b98ae; margin-top: 12px; text-align: center; }}
  </style>
</head>
<body>
  <div class="card">
    <h1>AirPad iPhone Setup</h1>
    <p class="sub">Trust your PC's local HTTPS certificate once so Safari enables low-latency WebSockets and Gyroscope Air-Mouse.</p>

    <div class="step">
      <div class="step-title">Step 1 · Download Certificate Profile</div>
      <div class="step-desc">Tap below and choose <strong>Allow</strong> when Safari asks to download a configuration profile.</div>
      <a class="btn btn-secondary" href="/ca.mobileconfig">Install AirPad Certificate Profile</a>
    </div>

    <div class="step">
      <div class="step-title">Step 2 · Trust on iPhone (One-time)</div>
      <div class="step-desc">
        1. Open <strong>Settings → General → VPN &amp; Device Management</strong> and install <em>AirPad Local HTTPS Trust</em>.<br/>
        2. Open <strong>Settings → General → About → Certificate Trust Settings</strong> and toggle on <em>AirPad Local Root CA</em>.
      </div>
    </div>

    <div class="step">
      <div class="step-title">Step 3 · Launch Wireless Trackpad</div>
      <div class="step-desc">Once trusted, open AirPad over encrypted HTTPS:</div>
      <a class="btn btn-primary" href="{https_url}">Open AirPad Trackpad</a>
    </div>

    <p class="note">You can also download the raw <a href="/ca.cer" style="color:#93c5fd;">.cer certificate</a>.</p>
  </div>
</body>
</html>"#
    );

    Html(html)
}

#[allow(dead_code)]
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let m = secs / 60;
    let s = secs % 60;
    format!("{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::mock::{MockEvent, RecordingInputBackend};
    use crate::protocol::{
        OP_KEY, OP_MOUSE_BTN, OP_MOVE, OP_PING, OP_PONG, OP_TEXT,
    };
    use futures_util::{SinkExt, StreamExt};
    use std::net::Ipv4Addr;
    use tokio::net::TcpListener;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as WsMsg;

    fn test_state() -> (Arc<AppState>, Arc<Mutex<Vec<MockEvent>>>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut cfg = Config::default();
        cfg.pairing_token = "testtoken99".to_string();
        let (status_tx, _) = watch::channel(StatusSnapshot { paused: false });
        let state = Arc::new(AppState {
            config: RwLock::new(cfg),
            paused: AtomicBool::new(false),
            active_session_id: AtomicU64::new(0),
            session_notify: Notify::new(),
            connected_client: RwLock::new(None),
            status_tx,
            input: Mutex::new(InputProcessor::new(
                Box::new(RecordingInputBackend::new(log.clone())),
                1.0,
                1.0,
            )),
            ca_der: vec![1, 2, 3, 4],
            mobileconfig: b"<plist>test</plist>".to_vec(),
        });
        (state, log)
    }

    async fn spawn_test_https_router(state: Arc<AppState>) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = build_https_router(state);
        tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        addr
    }

    async fn spawn_test_onboarding_router(state: Arc<AppState>) -> SocketAddr {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let router = build_http_onboarding_router(state);
        tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn serves_embedded_web_ui_and_onboarding_certs() {
        let (state, _) = test_state();
        let web_addr = spawn_test_https_router(state.clone()).await;
        let http_addr = spawn_test_onboarding_router(state).await;

        let client = reqwest::Client::new();
        let index_res = client
            .get(format!("http://{web_addr}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(index_res.status(), 200);
        let body = index_res.text().await.unwrap();
        assert!(body.contains("AirPad"));

        let mc_res = client
            .get(format!("http://{http_addr}/ca.mobileconfig"))
            .send()
            .await
            .unwrap();
        assert_eq!(mc_res.status(), 200);
        assert_eq!(mc_res.text().await.unwrap(), "<plist>test</plist>");
    }

    #[tokio::test]
    async fn rejects_invalid_token_with_4001() {
        let (state, _) = test_state();
        let addr = spawn_test_https_router(state).await;

        let (mut ws, _) = connect_async(format!("ws://{addr}/ws?token=wrong"))
            .await
            .unwrap();
        let msg = ws.next().await.unwrap().unwrap();
        match msg {
            WsMsg::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 4001),
            other => panic!("expected close 4001, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn processes_binary_packets_pings_pause_and_takeover() {
        let (state, log) = test_state();
        let addr = spawn_test_https_router(state.clone()).await;

        let (mut ws1, _) = connect_async(format!("ws://{addr}/ws?token=testtoken99"))
            .await
            .unwrap();

        // First message should be initial status JSON
        let first = ws1.next().await.unwrap().unwrap();
        assert!(first.into_text().unwrap().contains("\"paused\":false"));

        // Send Ping
        let mut ping = vec![OP_PING];
        ping.extend_from_slice(&123456_u64.to_le_bytes());
        ws1.send(WsMsg::Binary(ping)).await.unwrap();
        let pong = ws1.next().await.unwrap().unwrap().into_data();
        assert_eq!(pong[0], OP_PONG);
        assert_eq!(u64::from_le_bytes(pong[1..9].try_into().unwrap()), 123456);

        // Send Move, Click, Key, Text
        let mut mv = vec![OP_MOVE];
        mv.extend_from_slice(&5.0_f32.to_le_bytes());
        mv.extend_from_slice(&(-2.0_f32).to_le_bytes());
        ws1.send(WsMsg::Binary(mv)).await.unwrap();
        ws1.send(WsMsg::Binary(vec![OP_MOUSE_BTN, 0, 2])).await.unwrap();

        let mut key = vec![OP_KEY];
        key.extend_from_slice(&13_u16.to_le_bytes());
        key.push(2);
        ws1.send(WsMsg::Binary(key)).await.unwrap();

        let txt = "hi!";
        let mut txt_pkt = vec![OP_TEXT];
        txt_pkt.extend_from_slice(&(txt.len() as u16).to_le_bytes());
        txt_pkt.extend_from_slice(txt.as_bytes());
        ws1.send(WsMsg::Binary(txt_pkt)).await.unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;
        {
            let recorded = log.lock().unwrap();
            assert!(recorded.contains(&MockEvent::MoveRel(5, -2)));
            assert!(recorded.contains(&MockEvent::MouseButton(
                crate::protocol::MouseButton::Left,
                crate::protocol::PressAction::Click
            )));
            assert!(recorded.contains(&MockEvent::Key(13, crate::protocol::PressAction::Click)));
            assert!(recorded.contains(&MockEvent::Text("hi!".to_string())));
        }

        // Pause should trigger status update and ignore subsequent moves
        state.set_paused(true);
        let pause_msg = ws1.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(pause_msg.contains("\"paused\":true"));

        let prev_len = log.lock().unwrap().len();
        let mut mv2 = vec![OP_MOVE];
        mv2.extend_from_slice(&20.0_f32.to_le_bytes());
        mv2.extend_from_slice(&20.0_f32.to_le_bytes());
        ws1.send(WsMsg::Binary(mv2)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(40)).await;
        assert_eq!(log.lock().unwrap().len(), prev_len);

        // Second controller connects -> first controller replaced with close 4000
        let (mut ws2, _) = connect_async(format!("ws://{addr}/ws?token=testtoken99"))
            .await
            .unwrap();
        let ws2_init = ws2.next().await.unwrap().unwrap().into_text().unwrap();
        assert!(ws2_init.contains("\"paused\":true"));

        let close_msg = ws1.next().await.unwrap().unwrap();
        match close_msg {
            WsMsg::Close(Some(frame)) => assert_eq!(u16::from(frame.code), 4000),
            other => panic!("expected close 4000 on takeover, got {other:?}"),
        }
    }
}
