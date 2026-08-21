import { invoke } from "@tauri-apps/api/core";
import type {
  LinkAuditResult,
  WebEvidenceFetchRequest,
  WebEvidenceImportRequest,
  WebEvidenceListRequest,
  WebEvidenceListResult,
  WebEvidenceRecord,
  WebEvidenceSearchRequest,
  WebEvidenceSearchResult,
} from "../types";

export const auditLinks = (text: string) =>
  invoke<LinkAuditResult>("audit_links", { request: { text } });

export const listWebEvidence = (request: WebEvidenceListRequest = {}) =>
  invoke<WebEvidenceListResult>("list_web_evidence", { request });

export const fetchWebEvidence = (request: WebEvidenceFetchRequest) =>
  invoke<WebEvidenceRecord>("fetch_web_evidence", { request });

export const replayWebEvidence = (evidenceId: string) =>
  invoke<WebEvidenceRecord>("replay_web_evidence", { request: { evidence_id: evidenceId } });

export const searchWebEvidence = (request: WebEvidenceSearchRequest) =>
  invoke<WebEvidenceSearchResult>("search_web_evidence", { request });

export const startRenderedWebEvidence = (url: string) =>
  invoke<WebEvidenceRecord>("start_rendered_web_evidence", { request: { url } });

export const openWebEvidenceInDefaultBrowser = (url: string) =>
  invoke<WebEvidenceRecord>("open_web_evidence_in_default_browser", { request: { url } });

export const importOperatorEvidence = (request: WebEvidenceImportRequest) =>
  invoke<WebEvidenceRecord>("import_operator_evidence", { request });

export const resumeWebEvidenceInteraction = (evidenceId: string, confirmed: boolean) =>
  invoke<WebEvidenceRecord>("resume_web_evidence_interaction", {
    request: { evidence_id: evidenceId, confirmed },
  });

export const getWebEvidence = (evidenceId: string) =>
  invoke<WebEvidenceRecord>("get_web_evidence", { evidenceId });
