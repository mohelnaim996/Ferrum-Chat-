# Ferrum Chat

Monorepo:
- `backend`: Rust + Axum + sqlx + WebSocket
- `frontend`: Next.js 14 + TypeScript
- Postgres via Docker Compose (`chat_db`)

## Quick Start

1. Start DB
   - `docker compose up -d`
2. Backend
   - `cd backend`
   - `cp .env.example .env`
   - `cargo run`
3. Frontend
   - `cd frontend`
   - `cp .env.example .env.local`
   - `npm install`
   - `npm run dev`

## Implemented

- Health check: `GET /health`
- Auth:
  - `POST /auth/register`
  - `POST /auth/login`
- Rooms + history:
  - `GET /rooms`
  - `POST /rooms`
  - `GET /rooms/:id/messages?limit=50&before=<timestamp>`
- WebSocket:
  - `GET /ws?token=<JWT>`
  - Incoming: `join`, `message`, `typing`, `read`
  - Presence online/offline broadcasts

## Database Tables

- `users(id, username, password_hash, created_at)`
- `rooms(id, name, created_at)`
- `messages(id, room_id, user_id, content, created_at)`
- `message_reads(message_id, user_id, created_at)`
