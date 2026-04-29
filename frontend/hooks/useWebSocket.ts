"use client";

import { useEffect, useRef, useState } from "react";
import { ChatMessage } from "../types/chat";

type Presence = { user: string; status: "online" | "offline" };

export function useWebSocket(
  roomId: number | null,
  token: string | null,
  onMessageSaved: (message: ChatMessage) => void
) {
  const [presence, setPresence] = useState<Presence[]>([]);
  const [typingUser, setTypingUser] = useState<string | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const typingTimer = useRef<NodeJS.Timeout | null>(null);

  useEffect(() => {
    if (!roomId || !token) return;
    const base = (process.env.NEXT_PUBLIC_API_BASE || "http://localhost:8080").replace("http", "ws");
    const ws = new WebSocket(`${base}/ws?token=${token}`);
    wsRef.current = ws;

    ws.onopen = () => {
      ws.send(JSON.stringify({ type: "join", room_id: roomId }));
    };
    ws.onmessage = (event) => {
      const payload = JSON.parse(event.data);
      if (payload.type === "message") onMessageSaved(payload);
      if (payload.type === "presence") {
        setPresence((curr) => [...curr.filter((p) => p.user !== payload.user), payload]);
      }
      if (payload.type === "typing") {
        setTypingUser(payload.user);
        if (typingTimer.current) clearTimeout(typingTimer.current);
        typingTimer.current = setTimeout(() => setTypingUser(null), 2000);
      }
    };

    return () => {
      ws.close();
      wsRef.current = null;
    };
  }, [roomId, token, onMessageSaved]);

  const sendMessage = (content: string) => {
    if (!wsRef.current || !roomId) return;
    wsRef.current.send(JSON.stringify({ type: "message", room_id: roomId, content }));
  };

  const sendTyping = () => {
    if (!wsRef.current || !roomId) return;
    wsRef.current.send(JSON.stringify({ type: "typing", room_id: roomId }));
  };

  const sendRead = (messageId: number) => {
    if (!wsRef.current) return;
    wsRef.current.send(JSON.stringify({ type: "read", message_id: messageId }));
  };

  return { sendMessage, sendTyping, sendRead, presence, typingUser };
}
