import { invoke } from "@tauri-apps/api/core";
import type { LinkAuditResult } from "../types";

export const auditLinks = (text: string) =>
  invoke<LinkAuditResult>("audit_links", { request: { text } });
