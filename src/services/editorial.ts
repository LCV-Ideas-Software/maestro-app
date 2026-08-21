import { invoke } from "@tauri-apps/api/core";
import type {
  EditorialSessionResult,
  InitialAgentKey,
  MainSiteDraft,
  PromptAttachmentPayload,
  ResumableSessionInfo,
  SaveMainSiteDraftRequest,
} from "../types";

export const MAIN_SITE_SANITIZER_PROFILE = "mainsite_post_html.v1";

export type RunEditorialSessionRequest = {
  run_id: string;
  session_name: string;
  prompt: string;
  protocol_name: string;
  protocol_text: string;
  protocol_hash: string;
  initial_agent: InitialAgentKey;
  active_agents: InitialAgentKey[] | null;
  max_session_cost_usd: number | null;
  max_session_minutes: number | null;
  attachments: PromptAttachmentPayload[];
  links: string[];
};

export type ResumeEditorialSessionRequest = {
  run_id: string;
  protocol_name: string | null;
  protocol_text: string | null;
  protocol_hash: string | null;
  initial_agent: InitialAgentKey;
  active_agents: InitialAgentKey[] | null;
  max_session_cost_usd: number | null;
  max_session_minutes: number | null;
  attachments: PromptAttachmentPayload[] | null;
  links: string[] | null;
};

export const listResumableSessions = () =>
  invoke<ResumableSessionInfo[]>("list_resumable_sessions");

export const runEditorialSession = (request: RunEditorialSessionRequest) =>
  invoke<EditorialSessionResult>("run_editorial_session", { request });

export const resumeEditorialSession = (request: ResumeEditorialSessionRequest) =>
  invoke<EditorialSessionResult>("resume_editorial_session", { request });

export const stopEditorialSession = (runId: string) =>
  invoke<boolean>("stop_editorial_session", { runId });

export const loadMainSiteDraft = () => invoke<MainSiteDraft | null>("load_mainsite_draft");

export const saveMainSiteDraft = (request: SaveMainSiteDraftRequest) =>
  invoke<MainSiteDraft>("save_mainsite_draft", { request });
