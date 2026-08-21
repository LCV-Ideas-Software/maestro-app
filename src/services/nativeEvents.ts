import { listen } from "@tauri-apps/api/event";
import type { RuntimeBootstrapProgressEvent, WebEvidenceProgressEvent } from "../types";

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

export const listenToRuntimeBootstrapProgress = (
  handler: (payload: RuntimeBootstrapProgressEvent) => void,
) =>
  listen<RuntimeBootstrapProgressEvent>("runtime-bootstrap-progress", (event) =>
    handler(event.payload),
  );

export const listenToWebEvidenceProgress = (handler: (payload: WebEvidenceProgressEvent) => void) =>
  listen<WebEvidenceProgressEvent>("web-evidence-progress", (event) => handler(event.payload));
