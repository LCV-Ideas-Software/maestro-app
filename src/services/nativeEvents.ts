import { listen } from "@tauri-apps/api/event";

export type NativeLogPayload = {
  category?: string;
  context?: {
    run_id?: string;
    agent?: string;
    role?: string;
    cli?: string;
    status?: string;
    tone?: "ok" | "warn" | "blocked" | "error";
    elapsed_seconds?: number;
  };
};

export type NativeLogTone = NonNullable<NativeLogPayload["context"]>["tone"];

export const listenToNativeLogs = (handler: (payload: NativeLogPayload) => void) =>
  listen<NativeLogPayload>("maestro-log-event", (event) => handler(event.payload));
