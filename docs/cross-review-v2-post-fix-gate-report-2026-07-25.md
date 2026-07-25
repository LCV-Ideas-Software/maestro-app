# Cross-Review-v2 Post-Fix Gate Report

**Date:** 2026-07-25

**Cross-review runtime:** 4.5.29

**Caller/petitioner:** Codex

**Reviewed release:** Maestro Editorial AI 0.5.56

## Executive Summary

The review process found one real Maestro defect and materially improved the release:
completed-round progress could be persisted before round-scoped reviewer credits were
cleared. A crash in that window could resume the next round with stale credits. Maestro
now normalizes persisted round boundaries, clears round-scoped credits after boundary or
roster changes, persists the ordered roster in state schema v2, and preserves only
version-bound approvals from still-active independent peers.

After that repair, every peer in the final direct-review round returned raw `READY` and
no peer supplied a remaining code blocker. Cross-review-v2 nevertheless normalized all
five decisions to `NEEDS_EVIDENCE`. Therefore the post-fix result is substantively
unanimous but not formally converged. It must not be represented as
`outcome=converged`.

## Sessions

### `0ba5b870-bbb3-4f21-95a0-ddd720c65dd4`

- Purpose: evidence-rich final review of the original 0.5.56 patch.
- Material result: Claude found the completed-round crash-window defect.
- Correct finding: `turn_index == round_turn_count` could be rolled into the next round
  while retaining the previous round's `valid_round_agents`.
- Secondary concern: Claude requested proof that state persistence was atomic. The
  repository already supplied that proof in `editorial_io.rs`: temporary file created
  with `create_new`, `write_all`, `sync_all`, then bounded-retry `rename`.
- Disposition: boundary bug fixed and covered; corrupt authoritative JSON remains
  intentionally fail-closed.

### `2c0f1864-06a1-46db-9a14-85bb6c1a6f0a`

- Purpose: formal post-fix gate with explicit `lead_peer: perplexity`.
- Result: no provider call occurred. Budget preflight projected US$33.837241 against the
  explicit US$20 ceiling.
- Runtime inconsistency: despite the explicit supported `lead_peer: perplexity`
  argument, session polling reported Gemini as the lottery-selected relator.
- Requested fix: persist both `requested_lead_peer` and `effective_lead_peer`, honor a
  valid explicit non-caller lead, and add a regression around
  `session_start_unanimous`.

### `fa7cff95-8a52-442f-b3f4-b0ccd2b0cf6b`

- Purpose: direct full-panel review to avoid relator and attachment-delivery ambiguity.
- Round 1: all peers were blocked before paid calls by `evidence_preflight`. The packet
  contained raw command results in the draft, while the failure message claimed inline
  raw output was an accepted remedy. Re-submitting the same packet through `evidence`
  passed the gate.
- Round 2: Claude, Gemini, Grok and Perplexity returned raw `READY`. DeepSeek requested
  evidence because two separately labeled source excerpts were interpreted as one
  malformed Rust definition. The presentation was corrected.
- Round 3: Claude, Gemini, DeepSeek, Grok and Perplexity all returned raw `READY` with
  summary `No blocking objections remain.`
- Normalization:
  - Claude: `ready_peer_submitted_evidence_not_corroborated`
  - Gemini: `ready_peer_submitted_evidence_not_corroborated`
  - DeepSeek: `ready_peer_submitted_evidence_requires_verified_confidence`
  - Grok: `ready_without_evidence_sources`, then
    `ready_downgraded_to_needs_evidence`
  - Perplexity: `ready_without_evidence_sources`, then
    `ready_downgraded_to_needs_evidence`
- Final runtime state: blocked, not formally converged, no remaining code objection.

## Additional Runtime Evidence

- Session `e963fcdc-4f8b-4838-8e15-0334012b2d8b`: Perplexity said the production diff
  and validation attachment were unavailable, while the durable session metadata
  recorded the caller attachment, path, byte count and SHA-256.
- Session `4946caf9-5f6c-474d-820e-acbb762b761d`: every voting peer returned raw
  `READY`; two votes were normalized to `NEEDS_EVIDENCE` after composite-citation
  validation.
- Session `bca0f609-f749-4ef8-8701-ba51dfadb8e8`: generation was blocked before
  deliberation by unsettled provider accounting.

## Recommended Cross-Review-v2 Corrections

1. Honor and expose explicit relator selection. Add
   `requested_lead_peer`, `effective_lead_peer`, and selection reason to durable
   metadata.
2. Make the READY evidence contract internally consistent. Generated prompts and
   parser policy must agree on whether `READY` with inferred confidence may omit
   evidence and on how a peer references active caller-submitted evidence.
3. Correlate peer citations against the active caller evidence submission before
   classifying them as `peer_submitted_evidence_not_corroborated`.
4. Ensure Perplexity receives the same durable caller evidence content as the other
   adapters, rather than only metadata or a search-oriented prompt representation.
5. Align evidence-preflight diagnostics with actual accepted locations. If raw output
   in `draft` is rejected, the error must not claim that inline output is sufficient.
6. Give completed direct-review sessions a clear terminalization path. This session
   remained `outcome=null`, `health=blocked`, with no running job after three completed
   rounds.
7. Add adapter and end-to-end regressions using the exact three sessions above,
   preserving raw, parsed and normalized status separately.

## Maestro Validation

- `cargo test --locked`: 232 passed, 0 failed.
- `cargo clippy --locked --no-deps --all-targets -- -D warnings`: passed.
- Existing atomic-write regression: passed.
- New regressions: completed-round boundary, roster drift, corrupt authoritative state,
  accepted-state crash window, version-bound approvals and rejected-artifact isolation.
