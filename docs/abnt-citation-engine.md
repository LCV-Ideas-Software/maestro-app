# ABNT Citation Engine

Status: implemented; Rust validation runs in GitHub Actions for the consolidated release PR.
Last updated: 2026-08-21.

Maestro must apply the active editorial protocol as executable citation policy. The first profile is based on the attached Protocolo Editorial v1.10.0 and must treat ABNT formatting as a machine-checkable workflow, not as an optional style pass.

The private source protocol remains outside Git. Maestro stores operator-imported protocols locally and pins each session to a hash.

## Scope

The engine must support:

- ABNT NBR 10520:2023 citation formatting.
- ABNT NBR 6023 reference formatting.
- Direct quote, indirect quote, paraphrase, apud, footnote, and final-reference workflows.
- Mandatory locators for direct quotations.
- Detection of famous phrases in quotation marks as direct quotations.
- Compound surname preservation, including names that must not be truncated.
- Apparatus ordering required by the active protocol.
- Bibliographic quarantine for unverified or risky sources.
- Wikipedia and other prohibited-source handling according to the active protocol.
- Semantic diff of citation changes.

## Citation Inputs

Each citation candidate should be represented as structured data before formatting:

```json
{
  "schema_version": "citation.v1",
  "claim_id": "claim-001",
  "citation_type": "direct_quote | indirect_quote | paraphrase | apud | generic_mention",
  "author_display": "Sobrenome, Nome",
  "author_key": "SOBRENOME",
  "year": "2026",
  "locator": "p. 12",
  "source_id": "source-001",
  "source_access": "full_document_opened | excerpt_consulted | consolidated_memory | contextual_inference | unverified_hypothesis",
  "verification_status": "verified | needs_evidence | quarantined",
  "risk_if_wrong": "low | medium | high"
}
```

No direct quote may become publishable without a valid locator and a verified or explicitly operator-provided source.

The complete session manifest uses `citation_manifest.v1`, pins the same protocol hash as the session, and contains `citations` plus `sources`. Verified sources require a SHA-256 verification fingerprint; online sources also require URL and access date. Source metadata is always operator/evidence supplied. The engine never fills a missing author, title, year, publisher, locator, URL, access date, or verification fingerprint by inference.

## Session transport

Attach the current manifest to the session as UTF-8 JSON. The recommended name is `citation-manifest.json`; `manifesto-citacoes.json` is also recognized. An optional prior version named with `previous` or `anterior` is used only for semantic diff.

Start from [`docs/examples/citation-manifest.example.json`](examples/citation-manifest.example.json). Replace both placeholder SHA-256 values with the real hash of the session protocol and the verified source before attaching it; do not copy placeholder metadata into production evidence.

The attachment is persisted in the existing session evidence ledger and reused on resume. Exactly one current and at most one previous manifest are accepted. A file explicitly named as a citation manifest fails closed when its JSON or schema is invalid. Other JSON attachments remain unrelated evidence and are ignored by this engine.

When an article contains author-date citations but no structured manifest, the standalone raw-text command still reports citation/reference shape defects and emits a machine-readable `structured_manifest_missing` blocker. In a production session, Maestro initializes a protocol-pinned empty manifest when none is attached. Text without bibliographic apparatus can then pass; any detected author-date citation, final reference, note marker, `apud`, or HTML citation/quotation must have a matching manifest entry or the session pauses before the next paid reviewer. This is intentional: raw prose cannot prove source access or verification.

## Outputs

The engine must generate:

- In-text citation text.
- Footnote text when the selected style requires it.
- Normalized ABNT reference.
- MainSite-compatible HTML.
- Pure Markdown.
- Citation audit table.
- Citation semantic diff.
- Machine-readable blockers.

The implemented command is `audit_abnt_citations`. It returns `maestro_peer.v1`, a stable audit ID, normalized in-text citation and footnote candidates per citation, normalized references, Markdown, escaped MainSite-compatible HTML list items, blockers, an audit table, and a semantic diff. Free-text detection is conservative; authoritative normalization and verification use the attached manifest.

## Maestro as Fourth Peer

Maestro is not only an orchestrator. It acts as a deterministic fourth peer:

- Claude, Codex, and Gemini produce editorial judgments.
- Maestro independently checks protocol gates, citation shape, link evidence, source freshness, quarantine status, and export structure.
- Final delivery requires unanimity across the active AI peers and `MaestroPeer: READY`.

If Maestro finds a protocol/citation/evidence blocker, it must mark its own peer status as `NOT_READY` or `NEEDS_EVIDENCE` and create the next round even if all three AIs say `READY`.

The production session gate now evaluates this result before `texto-final.md` is written. An unchanged `READY` answer cannot count while the deterministic gate still fails. Correctable text-only blockers use the same bounded corrective-review path. A blocker marked `needs_evidence`, or one whose correction necessarily mutates the immutable manifest, pauses the session before another reviewer is charged and instructs the operator to attach or replace the manifest, then resume; agents are never asked to mutate an attachment or invent metadata.

## Required Blockers

Maestro must block publication when:

- A direct quotation lacks a locator.
- A quotation cannot be tied to a verified source.
- A source is in bibliographic quarantine.
- A prohibited source is used as citation support.
- A reference is missing required ABNT fields.
- A compound surname or canonical author name is malformed.
- A final reference exists without body use, or a body citation lacks final reference.
- A weak link is used only to satisfy a count.
- The public final text contains protocol self-reference.

## CI fixtures

Golden fixtures must cover:

- Direct quote with page.
- Direct quote without page, blocked.
- Indirect quote.
- Paraphrase with source.
- Apud.
- Compound surnames.
- Online source with access date.
- Chapter/book/reference variants.
- Quarantined source.
- Prohibited source.
- Markdown export.
- MainSite HTML export.

Frontend command/panel coverage runs locally without invoking the Rust toolchain. Rust unit, clippy, and build validation are reserved for GitHub Actions on the consolidated PR.
