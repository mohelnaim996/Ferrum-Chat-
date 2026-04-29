export type Room = {
  id: number;
  name: string;
  created_at: string;
};

export type ChatMessage = {
  id: number;
  room_id: number;
  user_id: number;
  username: string;
  content: string;
  created_at: string;
};
