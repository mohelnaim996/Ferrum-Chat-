use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use bcrypt::{hash, verify, DEFAULT_COST};
use chrono::{DateTime, Utc};
use futures::{sink::SinkExt, stream::StreamExt};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::{mpsc, Mutex};
use tower_http::cors::CorsLayer;

#[derive(Clone)]
struct AppState {
    db: PgPool,
    jwt_secret: String,
    rooms: Arc<Mutex<HashMap<i64, Vec<SocketPeer>>>>,
}

#[derive(Clone)]
struct SocketPeer {
    username: String,
    tx: mpsc::UnboundedSender<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Claims {
    sub: i64,
    username: String,
    exp: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum IncomingWsMessage {
    #[serde(rename = "join")]
    Join { room_id: i64 },
    #[serde(rename = "message")]
    Chat { room_id: i64, content: String },
    #[serde(rename = "typing")]
    Typing { room_id: i64 },
    #[serde(rename = "read")]
    Read { message_id: i64 },
}

#[derive(Deserialize)]
struct WsQuery {
    token: String,
}

#[derive(Deserialize)]
struct MessagesQuery {
    limit: Option<i64>,
    before: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    name: String,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url =
        std::env::var("DATABASE_URL").unwrap_or("postgres://postgres:postgres@localhost:5432/chat_db".to_owned());
    let jwt_secret = std::env::var("JWT_SECRET").unwrap_or("dev-secret-change-me".to_owned());
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or("127.0.0.1:8080".to_owned());

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to postgres");

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("failed to run migrations");

    let state = AppState {
        db,
        jwt_secret,
        rooms: Arc::new(Mutex::new(HashMap::new())),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/ws", get(ws_handler))
        .route("/rooms", get(list_rooms).post(create_room))
        .route("/rooms/:id/messages", get(room_messages))
        .with_state(state)
        .layer(CorsLayer::permissive());

    let addr: SocketAddr = bind_addr.parse().expect("invalid BIND_ADDR");
    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind failed");
    println!("listening on {}", addr);
    axum::serve(listener, app).await.expect("server failed");
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

async fn register(State(state): State<AppState>, Json(payload): Json<RegisterRequest>) -> impl IntoResponse {
    let hashed = match hash(payload.password, DEFAULT_COST) {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "hash error").into_response(),
    };

    let res = sqlx::query!(
        "INSERT INTO users (username, password_hash) VALUES ($1, $2) RETURNING id",
        payload.username,
        hashed
    )
    .fetch_one(&state.db)
    .await;

    match res {
        Ok(row) => {
            let token = make_token(row.id, &payload.username, &state.jwt_secret);
            (StatusCode::CREATED, Json(AuthResponse { token })).into_response()
        }
        Err(_) => (StatusCode::CONFLICT, "username already exists").into_response(),
    }
}

async fn login(State(state): State<AppState>, Json(payload): Json<LoginRequest>) -> impl IntoResponse {
    let row = sqlx::query!(
        "SELECT id, username, password_hash FROM users WHERE username = $1",
        payload.username
    )
    .fetch_optional(&state.db)
    .await;

    let Some(user) = (match row {
        Ok(v) => v,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    }) else {
        return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
    };

    let is_valid = verify(payload.password, &user.password_hash).unwrap_or(false);
    if !is_valid {
        return (StatusCode::UNAUTHORIZED, "invalid credentials").into_response();
    }

    let token = make_token(user.id, &user.username, &state.jwt_secret);
    (StatusCode::OK, Json(AuthResponse { token })).into_response()
}

fn make_token(user_id: i64, username: &str, secret: &str) -> String {
    let claims = Claims {
        sub: user_id,
        username: username.to_owned(),
        exp: (Utc::now().timestamp() + 24 * 60 * 60) as usize,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap()
}

fn auth_from_headers(headers: &HeaderMap, secret: &str) -> Option<Claims> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?;
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|d| d.claims)
}

async fn list_rooms(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if auth_from_headers(&headers, &state.jwt_secret).is_none() {
        return (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response();
    }

    let rows = sqlx::query!("SELECT id, name, created_at FROM rooms ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await;

    match rows {
        Ok(r) => (StatusCode::OK, Json(r)).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    }
}

async fn create_room(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateRoomRequest>,
) -> impl IntoResponse {
    if auth_from_headers(&headers, &state.jwt_secret).is_none() {
        return (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response();
    }
    let row = sqlx::query!(
        "INSERT INTO rooms (name) VALUES ($1) RETURNING id, name, created_at",
        payload.name
    )
    .fetch_one(&state.db)
    .await;
    match row {
        Ok(r) => (StatusCode::CREATED, Json(r)).into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    }
}

async fn room_messages(
    State(state): State<AppState>,
    Path(room_id): Path<i64>,
    Query(query): Query<MessagesQuery>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if auth_from_headers(&headers, &state.jwt_secret).is_none() {
        return (StatusCode::UNAUTHORIZED, "missing or invalid token").into_response();
    }
    let limit = query.limit.unwrap_or(50).clamp(1, 200);

    let rows = if let Some(before) = query.before {
        sqlx::query!(
            r#"
            SELECT m.id, m.room_id, m.user_id, u.username, m.content, m.created_at
            FROM messages m
            JOIN users u ON u.id = m.user_id
            WHERE m.room_id = $1 AND m.created_at < $2
            ORDER BY m.created_at DESC
            LIMIT $3
            "#,
            room_id,
            before,
            limit
        )
        .fetch_all(&state.db)
        .await
    } else {
        sqlx::query!(
            r#"
            SELECT m.id, m.room_id, m.user_id, u.username, m.content, m.created_at
            FROM messages m
            JOIN users u ON u.id = m.user_id
            WHERE m.room_id = $1
            ORDER BY m.created_at DESC
            LIMIT $2
            "#,
            room_id,
            limit
        )
        .fetch_all(&state.db)
        .await
    };

    match rows {
        Ok(mut r) => {
            r.reverse();
            (StatusCode::OK, Json(r)).into_response()
        }
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "db error").into_response(),
    }
}

async fn ws_handler(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let claims = decode::<Claims>(
        &query.token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|d| d.claims);

    let Some(user) = claims else {
        return StatusCode::UNAUTHORIZED.into_response();
    };

    ws.on_upgrade(move |socket| websocket_task(state, socket, user))
}

async fn websocket_task(state: AppState, socket: WebSocket, user: Claims) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let mut joined_room: Option<i64> = None;

    while let Some(Ok(Message::Text(text))) = receiver.next().await {
        let parsed = serde_json::from_str::<IncomingWsMessage>(&text);
        let Ok(msg) = parsed else { continue };

        match msg {
            IncomingWsMessage::Join { room_id } => {
                joined_room = Some(room_id);
                let mut rooms = state.rooms.lock().await;
                rooms.entry(room_id).or_default().push(SocketPeer {
                    username: user.username.clone(),
                    tx: tx.clone(),
                });
                drop(rooms);
                broadcast_presence(&state, room_id, &user.username, "online").await;
            }
            IncomingWsMessage::Chat { room_id, content } => {
                if content.trim().is_empty() {
                    continue;
                }

                let saved = sqlx::query!(
                    "INSERT INTO messages (room_id, user_id, content) VALUES ($1, $2, $3) RETURNING id, created_at",
                    room_id,
                    user.sub,
                    content
                )
                .fetch_one(&state.db)
                .await;

                let Ok(saved_row) = saved else { continue };
                let payload = serde_json::json!({
                    "type": "message",
                    "id": saved_row.id,
                    "room_id": room_id,
                    "user_id": user.sub,
                    "username": user.username,
                    "content": saved_row.content,
                    "created_at": saved_row.created_at,
                });
                broadcast_to_room(&state, room_id, payload.to_string()).await;
            }
            IncomingWsMessage::Typing { room_id } => {
                let payload = serde_json::json!({
                    "type": "typing",
                    "room_id": room_id,
                    "user": user.username,
                });
                broadcast_to_room(&state, room_id, payload.to_string()).await;
            }
            IncomingWsMessage::Read { message_id } => {
                let _ = sqlx::query!(
                    "INSERT INTO message_reads (message_id, user_id) VALUES ($1, $2) ON CONFLICT (message_id, user_id) DO NOTHING",
                    message_id,
                    user.sub
                )
                .execute(&state.db)
                .await;
            }
        }
    }

    if let Some(room_id) = joined_room {
        let mut rooms = state.rooms.lock().await;
        if let Some(peers) = rooms.get_mut(&room_id) {
            peers.retain(|p| p.username != user.username);
        }
        drop(rooms);
        broadcast_presence(&state, room_id, &user.username, "offline").await;
    }
}

async fn broadcast_presence(state: &AppState, room_id: i64, username: &str, status: &str) {
    let payload = serde_json::json!({
        "type": "presence",
        "user": username,
        "status": status
    });
    broadcast_to_room(state, room_id, payload.to_string()).await;
}

async fn broadcast_to_room(state: &AppState, room_id: i64, text: String) {
    let rooms = state.rooms.lock().await;
    if let Some(peers) = rooms.get(&room_id) {
        for peer in peers {
            let _ = peer.tx.send(text.clone());
        }
    }
}
