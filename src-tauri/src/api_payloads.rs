// Modulo: src-tauri/src/api_payloads.rs
// Descricao: Provider API request payload builders + native attachment
// support detection extracted from lib.rs in v0.3.44 per
// `docs/code-split-plan.md` migration step 3 (provider API surfaces).
//
// This module owns the per-provider JSON shapes for the chat-completions
// request bodies (`openai_api_input`, `anthropic_api_user_content`,
// `gemini_api_user_parts`), the attachment-by-attachment dispatch table
// (`provider_supports_native_attachment` + 4 per-provider helpers + the
// 20 MiB payload cap), and the input-cost preflight estimator
// (`api_input_estimate_chars`). All consumed by the API peer runners in
// `provider_runners.rs` (openai/anthropic/gemini) and by
// `provider_deepseek.rs`.
//
// What's here (10 items):
//   - `pub(crate) const API_NATIVE_ATTACHMENT_MAX_FILE_BYTES: u64 =
//     20 * 1024 * 1024` — single-file cap on inline base64 payloads sent
//     in the API request body. Files above the cap are silently skipped
//     by the per-provider supported() helpers.
//   - `pub(crate) fn api_input_estimate_chars(prompt, attachments,
//     provider) -> usize` — sums prompt chars + per-attachment overhead
//     (base64 chars + filename chars + media-type chars + 96 bytes for
//     JSON envelope). Used by cost preflight to estimate input tokens
//     before spending API budget.
//   - `provider_supports_native_attachment(provider, entry) -> bool` —
//     dispatches to the 3 per-provider helpers; unknown provider → false.
//   - `openai_api_attachment_supported(entry) -> bool` — image OR file
//     (known document type), gated by payload cap.
//   - `openai_api_file_attachment_supported(entry) -> bool` — known
//     document attachment proxy.
//   - `anthropic_api_attachment_supported(entry) -> bool` — image OR
//     PDF, gated by payload cap.
//   - `gemini_api_attachment_supported(entry) -> bool` — image | audio
//     | video | PDF | text-like | known document, gated by payload cap.
//   - `attachment_within_native_payload_cap(entry) -> bool` — single
//     point of truth for the 20 MiB inline-payload limit.
//   - `pub(crate) fn openai_api_input(prompt, attachments) ->
//     Result<Value, String>` — Responses API input shape:
//     `[{"role":"user","content":[{type:input_text,text:...},
//     {type:input_image,image_url:...}, {type:input_file,filename,
//     file_data}]}]`. Skips attachments above payload cap.
//   - `pub(crate) fn anthropic_api_user_content(prompt, attachments) ->
//     Result<Value, String>` — Messages API user content shape:
//     `[{"type":"text",text:...}, {"type":"image","source":{"type":
//     "base64","media_type":..,"data":..}}, {"type":"document",
//     "source":..,"title":..}]`. Skips attachments above payload cap.
//   - `pub(crate) fn gemini_api_user_parts(prompt, attachments) ->
//     Result<Vec<Value>, String>` — generateContent parts shape:
//     `[{text:..}, {inline_data:{mime_type:..,data:..}}]`. Inline_data
//     entries gated by `gemini_api_attachment_supported`.
//
// What stayed in lib.rs:
//   - `AttachmentManifestEntry` struct lives in `session_evidence.rs`
//     and is consumed via `pub(crate)` cross-module imports here. Same
//     for the 10 attachment helpers (`is_image_attachment` etc.) and
//     the attachment payload helpers (`attachment_base64`,
//     `attachment_data_url`, `normalized_attachment_media_type`,
//     `attachment_payload_base64_chars`).
//
// v0.3.44 is a pure move: every signature, JSON key string, MIME type
// literal, and match arm is identical to the v0.3.43 lib.rs source
// (commit f7beeb7).

use serde_json::{json, Value};
use std::io::{self, Write};

use crate::session_evidence::{
    attachment_base64, attachment_data_url, attachment_payload_base64_chars, is_image_attachment,
    is_known_document_attachment, is_pdf_attachment, normalized_attachment_media_type,
    AttachmentManifestEntry,
};

pub(crate) const API_NATIVE_ATTACHMENT_MAX_FILE_BYTES: u64 = 20 * 1024 * 1024;

/// Enforce the native HTTP body caps against exactly the UTF-8 JSON reqwest
/// sends, including base64 and escaping, without allocating a second payload.
/// OpenAI's decoded document limit is separate from its total request limit.
pub(crate) fn validate_native_provider_payload(provider: &str, body: &Value) -> Result<(), String> {
    let mut limit = match provider {
        "openai" => Some(512_000_000),
        "anthropic" => Some(32_000_000),
        "gemini" => Some(100_000_000),
        "perplexity" => Some(32 * 1024 * 1024),
        "deepseek" => Some(48 * 1024 * 1024),
        // xAI documents a decoded per-image limit, not a real-time aggregate
        // HTTP body ceiling. Do not copy another provider's request limit.
        "grok" => None,
        _ => return Ok(()),
    };
    if provider == "gemini"
        && body
            .get("contents")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("parts").and_then(Value::as_array))
            .flatten()
            .filter_map(|part| {
                part.pointer("/inline_data/mime_type")
                    .and_then(Value::as_str)
            })
            .any(|mime| mime.starts_with("audio/") || mime.starts_with("video/"))
    {
        // The modality-specific native guides require inline audio/video
        // requests under 20 MB, despite the general 100 MB inline-file cap.
        // Keep the stricter documented request ceiling without uploading files.
        limit = Some(20_000_000);
    }
    if provider == "openai" {
        let file_bytes = body
            .get("input")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("input_file"))
            .filter_map(|item| item.get("file_data").and_then(Value::as_str))
            .map(|data| {
                let base64 = data.split_once(',').map_or(data, |(_, encoded)| encoded);
                let padding = base64.len() - base64.trim_end_matches('=').len();
                ((base64.len() as u64 / 4) * 3).saturating_sub(padding as u64)
            })
            .fold(0u64, u64::saturating_add);
        if file_bytes > 50_000_000 {
            return Err("OpenAI accepts at most 50 MB of combined native document bytes; reduce the selected files.".to_string());
        }
    }
    if provider == "grok" {
        for image in body
            .get("input")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("content").and_then(Value::as_array))
            .flatten()
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("input_image"))
            .filter_map(|item| item.get("image_url").and_then(Value::as_str))
        {
            if let Some((media, data)) = image
                .strip_prefix("data:")
                .and_then(|image| image.split_once(";base64,"))
            {
                if !matches!(media, "image/png" | "image/jpeg") {
                    return Err("Grok native images must be PNG or JPEG.".to_string());
                }
                let padding = data.len() - data.trim_end_matches('=').len();
                let decoded = ((data.len() as u64 / 4) * 3).saturating_sub(padding as u64);
                if decoded > API_NATIVE_ATTACHMENT_MAX_FILE_BYTES {
                    return Err(
                        "Grok accepts native images up to 20 MiB of decoded image bytes."
                            .to_string(),
                    );
                }
            }
        }
    }
    struct ByteCounter(u64);
    impl Write for ByteCounter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len() as u64);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = ByteCounter(0);
    serde_json::to_writer(&mut counter, body)
        .map_err(|_| "Failed to measure native provider request size.".to_string())?;
    if let Some(limit) = limit.filter(|limit| counter.0 > *limit) {
        return Err(format!(
            "{provider} native request is {} bytes, exceeding its {limit}-byte limit; reduce the selected attachments or prompt.",
            counter.0
        ));
    }
    Ok(())
}

pub(crate) fn api_input_estimate_chars(
    prompt: &str,
    attachments: &[AttachmentManifestEntry],
    provider: &str,
) -> usize {
    let attachment_chars = attachments
        .iter()
        .filter(|entry| provider_supports_native_attachment(provider, entry))
        .map(|entry| {
            attachment_payload_base64_chars(entry)
                + entry.file_name.chars().count()
                + normalized_attachment_media_type(entry).chars().count()
                + 96
        })
        .sum::<usize>();
    prompt.chars().count().saturating_add(attachment_chars)
}

fn provider_supports_native_attachment(provider: &str, entry: &AttachmentManifestEntry) -> bool {
    match provider {
        "openai" => openai_api_attachment_supported(entry),
        "anthropic" => anthropic_api_attachment_supported(entry),
        "gemini" => gemini_api_attachment_supported(entry),
        "grok" => grok_api_attachment_supported(entry),
        _ => false,
    }
}

fn grok_api_attachment_supported(entry: &AttachmentManifestEntry) -> bool {
    attachment_within_native_payload_cap(entry)
        && matches!(
            normalized_attachment_media_type(entry).as_str(),
            "image/png" | "image/jpeg"
        )
}

/// Describe the bytes actually included in this adapter, without treating
/// locally stored binary files or a bounded text preview as full native input.
pub(crate) fn api_attachment_delivery_note(
    provider: &str,
    attachments: &[AttachmentManifestEntry],
) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let mut note = String::from("\n\n## Actual API attachment delivery\n\nLocal file paths cannot be opened by this API peer. A text preview is bounded and does not prove access to the full file. Do not infer file contents from metadata; request evidence when needed contents are unavailable.\n");
    for entry in attachments {
        let delivery = if provider_supports_native_attachment(provider, entry) {
            "native payload included"
        } else if entry.inline_preview_chars > 0 {
            "metadata and bounded text preview only; no native file payload"
        } else {
            "metadata only; file contents unavailable in this request"
        };
        note.push_str(&format!("- {}: {delivery}.\n", entry.file_name));
    }
    note
}

fn openai_api_attachment_supported(entry: &AttachmentManifestEntry) -> bool {
    if !attachment_within_native_payload_cap(entry) {
        return false;
    }
    is_image_attachment(entry) || openai_api_file_attachment_supported(entry)
}

fn openai_api_file_attachment_supported(entry: &AttachmentManifestEntry) -> bool {
    is_known_document_attachment(entry)
}

fn anthropic_api_attachment_supported(entry: &AttachmentManifestEntry) -> bool {
    if !attachment_within_native_payload_cap(entry) {
        return false;
    }
    is_image_attachment(entry) || is_pdf_attachment(entry)
}

fn gemini_api_attachment_supported(entry: &AttachmentManifestEntry) -> bool {
    if !attachment_within_native_payload_cap(entry) {
        return false;
    }
    let media = normalized_attachment_media_type(entry);
    media.starts_with("text/")
        || matches!(
            media.as_str(),
            "image/png"
                | "image/jpeg"
                | "image/webp"
                | "image/heic"
                | "image/heif"
                | "audio/wav"
                | "audio/mp3"
                | "audio/aiff"
                | "audio/aac"
                | "audio/ogg"
                | "audio/flac"
                | "audio/mpeg"
                | "audio/m4a"
                | "audio/l16"
                | "audio/opus"
                | "audio/alaw"
                | "audio/mulaw"
                | "audio/webm"
                | "video/mp4"
                | "video/mpeg"
                | "video/mov"
                | "video/avi"
                | "video/x-flv"
                | "video/mpg"
                | "video/webm"
                | "video/wmv"
                | "video/3gpp"
                | "application/pdf"
                | "application/json"
                | "application/rtf"
                | "application/x-javascript"
                | "application/x-typescript"
                | "application/x-python-code"
                | "application/x-ipynb+json"
        )
}

fn attachment_within_native_payload_cap(entry: &AttachmentManifestEntry) -> bool {
    entry.size_bytes <= API_NATIVE_ATTACHMENT_MAX_FILE_BYTES
}

pub(crate) fn openai_api_input(
    prompt: &str,
    attachments: &[AttachmentManifestEntry],
) -> Result<Value, String> {
    let mut content = vec![json!({ "type": "input_text", "text": prompt })];
    for entry in attachments {
        if !attachment_within_native_payload_cap(entry) {
            continue;
        }
        if is_image_attachment(entry) {
            content.push(json!({
                "type": "input_image",
                "image_url": attachment_data_url(entry)?
            }));
        } else if openai_api_file_attachment_supported(entry) {
            content.push(json!({
                "type": "input_file",
                "filename": entry.file_name.as_str(),
                "file_data": attachment_data_url(entry)?
            }));
        }
    }
    Ok(json!([
        {
            "role": "user",
            "content": content
        }
    ]))
}

pub(crate) fn anthropic_api_user_content(
    prompt: &str,
    attachments: &[AttachmentManifestEntry],
) -> Result<Value, String> {
    let mut content = vec![json!({ "type": "text", "text": prompt })];
    for entry in attachments {
        if !attachment_within_native_payload_cap(entry) {
            continue;
        }
        if is_image_attachment(entry) {
            content.push(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": normalized_attachment_media_type(entry),
                    "data": attachment_base64(entry)?
                }
            }));
        } else if is_pdf_attachment(entry) {
            content.push(json!({
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": attachment_base64(entry)?
                },
                "title": entry.file_name.as_str()
            }));
        }
    }
    Ok(Value::Array(content))
}

pub(crate) fn gemini_api_user_parts(
    prompt: &str,
    attachments: &[AttachmentManifestEntry],
) -> Result<Vec<Value>, String> {
    let mut parts = vec![json!({ "text": prompt })];
    for entry in attachments {
        if gemini_api_attachment_supported(entry) {
            parts.push(json!({
                "inline_data": {
                    "mime_type": normalized_attachment_media_type(entry),
                    "data": attachment_base64(entry)?
                }
            }));
        }
    }
    Ok(parts)
}

pub(crate) fn grok_api_input(
    system_prompt: &str,
    prompt: &str,
    attachments: &[AttachmentManifestEntry],
) -> Result<Value, String> {
    let mut content = vec![json!({ "type": "input_text", "text": prompt })];
    for entry in attachments {
        if grok_api_attachment_supported(entry) {
            content
                .push(json!({ "type": "input_image", "image_url": attachment_data_url(entry)? }));
        }
    }
    Ok(json!([
        { "role": "system", "content": system_prompt },
        { "role": "user", "content": content }
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(name: &str, mime: &str) -> AttachmentManifestEntry {
        AttachmentManifestEntry {
            original_name: name.to_string(),
            file_name: name.to_string(),
            media_type: mime.to_string(),
            size_bytes: 100,
            sha256: String::new(),
            path: "unused-unsupported-attachment".to_string(),
            inline_preview_chars: 20,
            inline_preview_truncated: false,
        }
    }

    #[test]
    fn gemini_does_not_send_unsupported_office_blobs_or_infer_mime_from_extension() {
        for (name, mime) in [
            (
                "document.docx",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            ),
            (
                "sheet.xlsx",
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            ),
            (
                "slides.pptx",
                "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            ),
            ("notes.txt", "application/octet-stream"),
        ] {
            let entry = attachment(name, mime);
            assert!(!gemini_api_attachment_supported(&entry));
            assert_eq!(
                gemini_api_user_parts("manifest preview", &[entry.clone()])
                    .unwrap()
                    .len(),
                1
            );
            assert!(
                openai_api_attachment_supported(&entry),
                "OpenAI retains native document support"
            );
        }
        assert!(gemini_api_attachment_supported(&attachment(
            "notes.md",
            "text/markdown"
        )));
        assert!(gemini_api_attachment_supported(&attachment(
            "doc.pdf",
            "application/pdf"
        )));
    }

    #[test]
    fn gemini_only_sends_documented_native_media_formats() {
        for mime in ["image/gif", "audio/midi", "video/x-unknown-container"] {
            let entry = attachment("unsupported-media", mime);
            assert!(!gemini_api_attachment_supported(&entry));
            assert_eq!(
                gemini_api_user_parts("manifest preview", &[entry])
                    .unwrap()
                    .len(),
                1
            );
        }
        for mime in [
            "image/heic",
            "image/heif",
            "audio/opus",
            "audio/webm",
            "video/mov",
            "video/mp4",
        ] {
            assert!(
                gemini_api_attachment_supported(&attachment("native-media", mime)),
                "{mime}"
            );
        }
        let gif = attachment("image.gif", "image/gif");
        assert!(openai_api_attachment_supported(&gif));
        assert!(anthropic_api_attachment_supported(&gif));
    }

    #[test]
    fn native_body_limit_counts_aggregate_base64_overhead_and_json_envelope() {
        // Two 12 MB decoded files fit individually; their encoded data alone
        // reaches Claude's 32 MB request cap before the JSON envelope is added.
        let body = json!({ "messages": [{ "content": [
            { "source": { "data": "A".repeat(16_000_000) } },
            { "source": { "data": "A".repeat(16_000_000) } }
        ] }] });
        assert!(validate_native_provider_payload("anthropic", &body).is_err());
        assert!(validate_native_provider_payload("gemini", &body).is_ok());
    }

    #[test]
    fn inline_gemini_media_and_perplexity_enforce_their_native_body_caps() {
        let mut body = json!({ "contents": [{ "parts": [
            { "inline_data": { "mime_type": "audio/opus", "data": "A".repeat(10_000_000) } },
            { "inline_data": { "mime_type": "video/mp4", "data": "A".repeat(10_000_000) } }
        ] }] });
        assert!(validate_native_provider_payload("gemini", &body).is_err());
        body["contents"][0]["parts"].as_array_mut().unwrap().pop();
        assert!(validate_native_provider_payload("gemini", &body).is_ok());
        let agent = json!({ "input": "A".repeat(32 * 1024 * 1024) });
        assert!(validate_native_provider_payload("perplexity", &agent).is_err());
        assert!(validate_native_provider_payload("gemini", &agent).is_ok());
    }

    #[test]
    fn openai_document_limit_uses_combined_decoded_bytes() {
        let files = (0..3)
            .map(|_| {
                json!({ "type": "input_file",
            "file_data": "A".repeat(22_222_224) })
            })
            .collect::<Vec<_>>();
        let mut body = json!({ "input": [{ "content": files }] });
        assert!(validate_native_provider_payload("openai", &body).is_err());
        body["input"][0]["content"].as_array_mut().unwrap().pop();
        assert!(validate_native_provider_payload("openai", &body).is_ok());
    }

    #[test]
    fn grok_native_input_encodes_supported_images_and_preserves_text_preview() {
        let directory = crate::app_paths::sessions_dir();
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!(
            "grok-image-test-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
        let mut image = attachment("image.png", "image/png");
        image.path = path.to_string_lossy().into_owned();
        image.size_bytes = 8;
        let mut text = attachment("notes.txt", "text/plain");
        text.inline_preview_chars = 14;
        let prompt = format!(
            "manifest and preview: useful content{}",
            api_attachment_delivery_note("grok", &[image.clone(), text.clone()])
        );
        let body =
            json!({ "input": grok_api_input("system", &prompt, &[image.clone(), text]).unwrap() });
        assert_eq!(
            body.pointer("/input/1/content/1/type")
                .and_then(Value::as_str),
            Some("input_image")
        );
        assert_eq!(
            body.pointer("/input/1/content/1/image_url")
                .and_then(Value::as_str),
            Some("data:image/png;base64,iVBORw0KGgo=")
        );
        let delivered_prompt = body
            .pointer("/input/1/content/0/text")
            .and_then(Value::as_str)
            .unwrap();
        assert!(delivered_prompt.contains("useful content"));
        assert!(delivered_prompt.contains("notes.txt: metadata and bounded text preview only"));
        assert!(validate_native_provider_payload("grok", &body).is_ok());
        for mime in ["image/jpeg", "image/jpg", "image/png"] {
            assert!(grok_api_attachment_supported(&attachment(
                "native-image",
                mime
            )));
        }
        for mime in ["image/gif", "image/webp", "application/pdf", "text/plain"] {
            assert!(!grok_api_attachment_supported(&attachment(
                "preview-only",
                mime
            )));
        }
        image.size_bytes = API_NATIVE_ATTACHMENT_MAX_FILE_BYTES + 1;
        let oversized = grok_api_input("system", "metadata remains", &[image]).unwrap();
        assert_eq!(
            oversized
                .pointer("/1/content")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            1
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn grok_raw_image_cap_is_distinct_from_base64_and_deepseek_body_limit() {
        // 20 MiB decoded bytes encode to more than 20 MiB of request text.
        let raw_bytes = API_NATIVE_ATTACHMENT_MAX_FILE_BYTES as usize;
        let mut data = "A".repeat(raw_bytes.div_ceil(3) * 4);
        data.replace_range(data.len() - 1.., "=");
        let mut body = json!({ "input": [{ "role": "user", "content": [
            { "type": "input_image", "image_url": format!("data:image/png;base64,{data}") }
        ] }] });
        assert!(validate_native_provider_payload("grok", &body).is_ok());
        body["input"][0]["content"][0]["image_url"] = json!(format!(
            "data:image/png;base64,{}",
            "A".repeat((raw_bytes + 1).div_ceil(3) * 4)
        ));
        assert!(validate_native_provider_payload("grok", &body)
            .unwrap_err()
            .contains("decoded image bytes"));
        let text_request = json!({ "messages": [{ "content": "A".repeat(48*1024*1024) }] });
        assert!(validate_native_provider_payload("deepseek", &text_request).is_err());
    }

    #[test]
    fn text_only_providers_describe_preview_custody_without_rejecting_it() {
        let text = attachment("notes.md", "text/markdown");
        let mut binary = attachment("scan.pdf", "application/pdf");
        binary.inline_preview_chars = 0;
        for provider in ["deepseek", "perplexity"] {
            let note = api_attachment_delivery_note(provider, &[text.clone(), binary.clone()]);
            assert!(note.contains("notes.md: metadata and bounded text preview only"));
            assert!(note.contains("scan.pdf: metadata only; file contents unavailable"));
            assert!(!note.contains("native payload included"));
            assert!(validate_native_provider_payload(
                provider,
                &json!({ "input": format!("Existing useful preview{note}") })
            )
            .is_ok());
        }
    }
}
