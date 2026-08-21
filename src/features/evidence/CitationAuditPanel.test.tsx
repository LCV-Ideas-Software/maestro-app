import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { CitationAuditCitation, CitationAuditResult } from "../../types";
import { CitationAuditPanel } from "./CitationAuditPanel";

const readyCitation: CitationAuditCitation = {
  schema_version: "citation.v1",
  claim_id: "claim-1",
  citation_type: "direct_quote",
  author_display: "Silva, Maria",
  author_key: "SILVA",
  year: "2026",
  locator: "p. 12",
  source_id: "source-1",
  source_access: "full_document_opened",
  verification_status: "verified",
  risk_if_wrong: "medium",
  original_text: "(SILVA, 2026, p. 12)",
  normalized_text: "(SILVA, 2026, p. 12)",
};

const readyResult: CitationAuditResult = {
  schema_version: "citation_audit.v1",
  audit_id: "audit-1",
  checked_at: "2026-08-21T18:00:00Z",
  protocol_hash: "sha256-protocol",
  maestro_peer_status: "ready",
  citations: [readyCitation],
  normalized_references: ["SILVA, Maria. Obra. São Paulo: Editora, 2026."],
  markdown_references: ["SILVA, Maria. **Obra**. São Paulo: Editora, 2026."],
  html_references: ["<p>SILVA, Maria. <strong>Obra</strong>. São Paulo: Editora, 2026.</p>"],
  blockers: [],
  audit_table_markdown: "| Claim | Fonte |\n| --- | --- |\n| claim-1 | source-1 |",
  semantic_diff: "Nenhuma alteração semântica.",
};

describe("CitationAuditPanel", () => {
  it("shows READY as a mechanical verdict, never as AI approval", () => {
    render(<CitationAuditPanel result={readyResult} isAuditing={false} />);

    expect(screen.getByText("MaestroPeer: READY mecânico")).toBeInTheDocument();
    expect(
      screen.getByText("O veredito é determinístico, não uma aprovação por consenso de IA."),
    ).toBeInTheDocument();
    expect(screen.getByText("Silva, Maria")).toBeInTheDocument();
  });

  it("renders a NOT_READY blocker with its claim and source", () => {
    const result: CitationAuditResult = {
      ...readyResult,
      maestro_peer_status: "not_ready",
      blockers: [
        {
          code: "DIRECT_QUOTE_WITHOUT_LOCATOR",
          message: "Citação direta sem localizador verificável.",
          severity: "blocking",
          claim_id: "claim-1",
          source_id: "source-1",
          excerpt: "Trecho citado",
          needs_evidence: false,
        },
      ],
    };

    render(<CitationAuditPanel result={result} isAuditing={false} />);

    expect(screen.getByText("MaestroPeer: NOT_READY")).toBeInTheDocument();
    expect(screen.getByText("DIRECT_QUOTE_WITHOUT_LOCATOR")).toBeInTheDocument();
    expect(screen.getByText("Claim claim-1 · Fonte source-1")).toBeInTheDocument();
  });

  it("keeps NEEDS_EVIDENCE visible for quarantined citations", () => {
    const result: CitationAuditResult = {
      ...readyResult,
      maestro_peer_status: "needs_evidence",
      citations: [{ ...readyCitation, verification_status: "quarantined" }],
      blockers: [
        {
          code: "SOURCE_NEEDS_EVIDENCE",
          message: "A fonte precisa de comprovação adicional.",
          severity: "warning",
          source_id: "source-1",
          needs_evidence: true,
        },
      ],
    };

    render(<CitationAuditPanel result={result} isAuditing={false} />);

    expect(screen.getByText("MaestroPeer: NEEDS_EVIDENCE")).toBeInTheDocument();
    expect(screen.getByText("SOURCE_NEEDS_EVIDENCE")).toBeInTheDocument();
    expect(screen.getAllByText("NEEDS_EVIDENCE").length).toBeGreaterThan(0);
    expect(screen.getByText("quarantined")).toBeInTheDocument();
  });
});
