# Link Integrity Engine

Status: implemented; native verification is performed by GitHub Actions.
Date: 2026-08-21.

AI-generated texts often contain broken, invented, misleading, stale, or weak links. Maestro must treat link integrity as a central editorial gate and must take unresolved link problems back into cross-review.

## Responsibilities

The engine:

- Extracts Markdown links, HTML anchors, and bare URLs from the active editorial text; imported or PDF-derived text is covered once it enters that text surface.
- Preserve anchor text and surrounding citation/claim context.
- Normalize URLs without hiding meaningful changes.
- Reuses the SSRF-safe Web Evidence Engine for bounded public `GET` validation and records interaction-required states for rendered or operator-assisted follow-up.
- Capture redirect chains, final URL, status, content type, content hash, timestamp, and error class.
- Classifies deterministic transport, access, redirect, and content-type failures without pretending that HTTP success proves a claim.
- Keeps mechanically reachable links pending until an explicit editorial decision records whether the evidence supports the claim.
- Sanitize output links for MainSite rendering.
- Proposes replacement, removal, and rewording candidates without applying semantic changes automatically.
- Feeds unresolved rows and candidates into the existing final-release corrective-review gate.

## Link Classes

Every link receives one class:

- `verified_supports_claim`
- `verified_but_weak`
- `redirected_verified`
- `content_type_mismatch`
- `not_found`
- `forbidden`
- `auth_required`
- `captcha_required`
- `paywall`
- `timeout`
- `dns_error`
- `tls_error`
- `malformed`
- `suspected_hallucination`
- `quarantined`

Only `verified_supports_claim` and explicitly accepted `redirected_verified` links may remain in a publishable final text without a blocker note.

## Evidence Record

```json
{
  "schema_version": "link_evidence.v1",
  "link_id": "link-001",
  "source_artifact": "operator/current-editor",
  "source_fingerprint": "<sha256_of_source_text>",
  "anchor_text": "artigo citado",
  "surrounding_text": "trecho curto ao redor do link",
  "original_url": "https://example.invalid/path",
  "normalized_url": "https://example.invalid/path",
  "final_url": "https://example.invalid/path",
  "redirect_chain": [],
  "http_status": 200,
  "content_type": "text/html",
  "sha256": "<content_hash>",
  "checked_at": "2026-04-26T00:00:00Z",
  "claim_supported": true,
  "classification": "verified_supports_claim",
  "correction_candidates": [],
  "cross_review_status": "not_needed | pending | accepted | rejected",
  "review_decision": "accept | reject | quarantine",
  "reviewed_by": "operator",
  "review_note": "decisao editorial registrada",
  "reviewed_at": "2026-08-21T00:00:00Z"
}
```

Records are stored under `data/evidence/link-integrity/`. Review writeback is append-only in `events.ndjson`, requires an allowed reviewer identity and a substantive note, and uses optimistic checks against the normalized URL and content SHA-256. Link identity includes the complete source fingerprint and local claim context, so a decision cannot migrate to a different assertion that reuses the same URL. A later mechanical audit preserves a decision only while the source, assertion, URL, and content hash remain unchanged.

## Sanitization

For MainSite-compatible HTML, Maestro must:

- Preserve safe `http`, `https`, and `mailto` links.
- Reject `javascript:`, unsafe data URLs, malformed URLs, and suspicious control characters.
- Normalize internal LCV-family links according to the MainSite reader behavior.
- Add `target="_blank"` and `rel="noopener noreferrer"` to external non-YouTube links before save.
- Keep YouTube embed/link behavior compatible with PostEditor and PostReader.
- Re-run the sanitizer after any automatic correction.

Explicit `ftp`, `ftps`, `tel`, executable, data, file, blob, and unknown schemes are not publishable. The integrity extractor records link-like unsupported protocols as malformed so they cannot bypass the final gate.

## Correction Workflow

When a link fails:

1. Maestro records the failure.
2. Maestro searches for official or stronger alternatives.
3. Maestro proposes replacement, removal, or textual rewording.
4. An identified reviewer accepts, rejects, or quarantines the evidence with a recorded rationale; the existing editorial round receives the structured evidence packet when revision is required.
5. Maestro rechecks any accepted replacement mechanically.
6. The final text remains blocked while a failure or pending review exists.

If no reliable replacement exists, the claim must be removed, rewritten without the link, or explicitly marked as unresolved in a non-publicable draft.

## Cross-Review Integration

Broken, invented, weak, or mismatched links create `NEEDS_EVIDENCE` unless the issue is purely syntactic and Maestro can repair it deterministically.

The cross-review prompt must include:

- Original link.
- Failure class.
- Anchor and surrounding text.
- Claim the link was supposed to support.
- Redirect/fetch evidence.
- Proposed correction candidates.
- Maestro's recommendation.

The final-release gate remains blocked until every link is mechanically valid and no editorial review is pending. Manual acceptance is recorded as an identified decision; it is not represented as unanimous AI review.

## Current boundary

- The engine does not infer semantic support from status code, title, or content hash.
- Correction candidates never alter the article automatically.
- Candidate lookup updates only the candidate list under an atomic read-modify-write lock; it cannot erase or overwrite a concurrent editorial decision.
- More than 30 link occurrences fail closed before any partial network audit; the engine never reports a truncated set as complete.
- Rendered/browser-assisted evidence stays in the Web Evidence workflow and requires explicit operator custody.
- Rust compilation, Clippy, native tests, and Windows portable validation run only in GitHub Actions for this workstream.
