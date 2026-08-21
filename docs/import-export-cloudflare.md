# Import, Export, and Cloudflare D1

Status: public shared-chat import, Markdown/MainSite HTML/PDF export, and API-first D1 publication implemented.
Last reviewed: 2026-08-21.

## Shared Chat Links

Maestro classifies and imports public shared chat snapshots from the three provider web apps:

- ChatGPT: `https://chatgpt.com/share/<conversation-id>`.
- Gemini: `https://g.co/gemini/share/...` and canonical Gemini shared-chat URLs.
- Claude: Claude shared chat links created through the Claude sharing flow.

Import is evidence-oriented, not blind scraping:

1. Normalize the URL and provider.
2. Fetch the public snapshot through the native evidence path; when login, consent, CAPTCHA, or unsupported dynamic rendering prevents trustworthy extraction, stop with an operator-action handoff.
3. Extract prompt, response text, artifacts when visible, timestamp hints, and source URL.
4. Convert to normalized Markdown plus a JSON provenance record.
5. Store import evidence under ignored local session data.
6. Never treat a shared chat as a verified source for factual claims; it is an input artifact.

If a provider changes the share page structure, the importer must fail with a diagnostic event rather than fabricating content.

## File Formats

Implemented read and write paths:

- Pure Markdown.
- Markdown plus trusted HTML blocks.
- PDF evidence attachment/capture and PDF export for final delivery through the system print dialog. Automatic PDF text extraction is not inferred when no trusted extractor is configured.
- MainSite-compatible HTML through the PostEditor parity module.

Markdown and PDF conversions must preserve provenance metadata separately from the public final text.

## Web Evidence

Shared-chat imports and source verification depend on the Web Evidence Engine in `docs/web-evidence-engine.md`.

If a provider or website requires human interaction, Maestro must pause, open an assisted browser window or the system default browser, let the operator resolve CAPTCHA/login/consent/download prompts, and then import or continue from the resulting artifact. It must not use hidden browser-profile access or cookie extraction.

## Cloudflare D1

Target:

```text
example_db.mainsite_posts
```

Maestro may read, preview, insert, and update records, but the write path is gated by:

- Local credentials stored only in ignored runtime vault/config files.
- Cloudflare API as the primary execution path for every D1 operation.
- A fail-closed stop when the primary API path is unavailable. The release does not claim an unaudited Wrangler write fallback; selecting fallback permission changes the diagnostic but never improvises a remote mutation.
- Operator confirmation before any remote write.
- Dry-run preview containing SQL intent, affected row, and sanitized HTML diff.
- PostEditor parity output.
- MainSite sanitizer pass.
- `PostReader` compatibility fixtures.

For a local Windows desktop app, D1 access uses Cloudflare's API. Wrangler remains useful for separately authorized diagnostics, but this release deliberately has no Wrangler publication fallback: an API failure is reported and no second write path is attempted. API tokens, account IDs, database IDs, and Cloudflare credentials must never be committed.
