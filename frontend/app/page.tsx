"use client";

import { FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useWebSocket } from "../hooks/useWebSocket";
import { ChatMessage, Room } from "../types/chat";

const API = process.env.NEXT_PUBLIC_API_BASE || "http://localhost:8080";

export default function Page() {
  const [token, setToken] = useState<string | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [rooms, setRooms] = useState<Room[]>([]);
  const [roomName, setRoomName] = useState("");
  const [activeRoom, setActiveRoom] = useState<number | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [typingCooldown, setTypingCooldown] = useState(0);

  useEffect(() => {
    const stored = localStorage.getItem("token");
    if (stored) setToken(stored);
  }, []);

  const onMessageSaved = useCallback((m: ChatMessage) => {
    setMessages((curr) => [...curr, m]);
  }, []);

  const { sendMessage, sendTyping, sendRead, presence, typingUser } = useWebSocket(activeRoom, token, onMessageSaved);

  const headers = useMemo(
    () =>
      token
        ? {
            Authorization: `Bearer ${token}`,
            "Content-Type": "application/json"
          }
        : undefined,
    [token]
  );

  async function auth(path: "register" | "login", e: FormEvent) {
    e.preventDefault();
    const res = await fetch(`${API}/auth/${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password })
    });
    if (!res.ok) return alert("Auth failed");
    const data = await res.json();
    localStorage.setItem("token", data.token);
    setToken(data.token);
  }

  async function loadRooms() {
    if (!headers) return;
    const res = await fetch(`${API}/rooms`, { headers });
    if (!res.ok) return;
    const data = await res.json();
    setRooms(data);
  }

  async function createRoom() {
    if (!headers || !roomName.trim()) return;
    const res = await fetch(`${API}/rooms`, {
      method: "POST",
      headers,
      body: JSON.stringify({ name: roomName })
    });
    if (!res.ok) return;
    setRoomName("");
    loadRooms();
  }

  async function loadHistory(roomId: number) {
    if (!headers) return;
    const res = await fetch(`${API}/rooms/${roomId}/messages?limit=50`, { headers });
    if (!res.ok) return;
    const data = await res.json();
    setMessages(data);
    for (const msg of data) sendRead(msg.id);
  }

  useEffect(() => {
    if (token) loadRooms();
  }, [token]);

  if (!token) {
    return (
      <main style={{ maxWidth: 420, margin: "100px auto", display: "grid", gap: 12 }}>
        <h1>Ferrum Chat</h1>
        <form onSubmit={(e) => auth("register", e)} style={{ display: "grid", gap: 8 }}>
          <input value={username} onChange={(e) => setUsername(e.target.value)} placeholder="username" />
          <input value={password} onChange={(e) => setPassword(e.target.value)} type="password" placeholder="password" />
          <button type="submit">Register</button>
        </form>
        <form onSubmit={(e) => auth("login", e)} style={{ display: "grid", gap: 8 }}>
          <button type="submit">Login</button>
        </form>
      </main>
    );
  }

  return (
    <main style={{ display: "grid", gridTemplateColumns: "280px 1fr", minHeight: "100vh" }}>
      <aside style={{ borderRight: "1px solid #243046", padding: 16 }}>
        <h3>Rooms</h3>
        <div style={{ display: "grid", gap: 8 }}>
          {rooms.map((room) => (
            <button
              key={room.id}
              onClick={() => {
                setActiveRoom(room.id);
                loadHistory(room.id);
              }}
            >
              {room.name}
            </button>
          ))}
        </div>
        <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
          <input value={roomName} onChange={(e) => setRoomName(e.target.value)} placeholder="new room" />
          <button onClick={createRoom}>Create</button>
        </div>
      </aside>
      <section style={{ display: "grid", gridTemplateRows: "1fr auto", padding: 16 }}>
        <div style={{ overflowY: "auto", display: "grid", gap: 8 }}>
          {messages.map((m) => (
            <div key={m.id} style={{ background: "#1a2231", padding: 8, borderRadius: 6 }}>
              <strong>{m.username}: </strong>
              {m.content}
              <small style={{ marginLeft: 8, opacity: 0.8 }}>✓✓</small>
            </div>
          ))}
          {typingUser ? <small>{typingUser} is typing...</small> : null}
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!draft.trim()) return;
            sendMessage(draft);
            setDraft("");
          }}
          style={{ display: "flex", gap: 8 }}
        >
          <input
            style={{ flex: 1 }}
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              if (Date.now() > typingCooldown) {
                sendTyping();
                setTypingCooldown(Date.now() + 2000);
              }
            }}
            onKeyDown={(e) => {
              if (e.key === "Enter") return;
            }}
            placeholder={activeRoom ? "Type a message..." : "Pick a room first"}
          />
          <button type="submit">Send</button>
        </form>
        <div style={{ marginTop: 8 }}>
          {presence
            .filter((p) => p.status === "online")
            .map((p) => p.user)
            .join(", ")}
        </div>
      </section>
    </main>
  );
}
