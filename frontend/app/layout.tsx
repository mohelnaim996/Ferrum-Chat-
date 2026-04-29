import "./globals.css";
import { ReactNode } from "react";

export const metadata = {
  title: "Ferrum Chat",
  description: "Realtime chat with Rust + Next.js"
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
