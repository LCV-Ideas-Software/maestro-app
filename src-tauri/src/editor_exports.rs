//! Native portable custody for PostEditor exports. Browser downloads do not
//! inherit Maestro's executable-relative data directory.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::app_paths::{checked_data_child_path, data_dir};
use crate::editorial_io::{read_text_file, write_text_file};

const MAX_EXPORT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum EditorExportFormat {
    Markdown,
    Html,
    Pdf,
}

impl EditorExportFormat {
    fn directory(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Pdf => "pdf",
        }
    }

    fn content_suffix(self) -> &'static str {
        match self {
            Self::Markdown => ".md",
            Self::Html => ".mainsite.html",
            Self::Pdf => ".pdf",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EditorExportRequest {
    format: EditorExportFormat,
    filename: String,
    content: String,
}

#[derive(Serialize)]
pub(crate) struct EditorExportResult {
    relative_path: String,
    bytes: usize,
}

fn validated_export_path(request: &EditorExportRequest) -> Result<PathBuf, String> {
    if request.content.is_empty() || request.content.len() > MAX_EXPORT_BYTES {
        return Err(format!(
            "export content must contain 1 to {MAX_EXPORT_BYTES} bytes"
        ));
    }
    let provenance = request.filename.ends_with(".provenance.json");
    let suffix = if provenance {
        ".provenance.json"
    } else if request.format == EditorExportFormat::Pdf {
        // The existing Windows print dialog owns PDF creation and its selected
        // destination. This command persists only the separate PDF sidecar.
        return Err("PDF bytes must be saved through the native print dialog".to_string());
    } else {
        request.format.content_suffix()
    };
    let base = request
        .filename
        .strip_suffix(suffix)
        .filter(|base| {
            !base.is_empty()
                && base.len() <= 80
                && !base.starts_with('-')
                && !base.ends_with('-')
                && base.bytes().all(|character| {
                    character.is_ascii_lowercase()
                        || character.is_ascii_digit()
                        || character == b'-'
                })
        })
        .ok_or_else(|| "export filename does not match the supported format".to_string())?;
    if matches!(base, "con" | "prn" | "aux" | "nul")
        || (base.len() == 4
            && (base.starts_with("com") || base.starts_with("lpt"))
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("export filename is reserved by Windows".to_string());
    }
    if provenance {
        let payload: serde_json::Value = serde_json::from_str(&request.content)
            .map_err(|_| "export provenance is not valid JSON".to_string())?;
        let expected_document = format!("{base}{}", request.format.content_suffix());
        if payload
            .get("schema_version")
            .and_then(serde_json::Value::as_str)
            != Some("maestro.export-provenance.v1")
            || payload.get("format").and_then(serde_json::Value::as_str)
                != Some(request.format.directory())
            || payload
                .pointer("/document/filename")
                .and_then(serde_json::Value::as_str)
                != Some(expected_document.as_str())
        {
            return Err("export provenance does not match the document and format".to_string());
        }
    }
    checked_data_child_path(
        &data_dir()
            .join("exports")
            .join(request.format.directory())
            .join(format!("{base}{suffix}")),
    )
}

fn persist_editor_export_inner(request: EditorExportRequest) -> Result<EditorExportResult, String> {
    let path = validated_export_path(&request)?;
    write_text_file(&path, &request.content)?;
    if read_text_file(&path)? != request.content {
        return Err("portable export readback did not match the written content".to_string());
    }
    Ok(EditorExportResult {
        relative_path: format!(
            "exports/{}/{}",
            request.format.directory(),
            request.filename
        ),
        bytes: request.content.len(),
    })
}

#[tauri::command(async)]
pub(crate) fn persist_editor_export(
    request: EditorExportRequest,
) -> Result<EditorExportResult, String> {
    persist_editor_export_inner(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(format: EditorExportFormat, filename: &str, content: &str) -> EditorExportRequest {
        EditorExportRequest {
            format,
            filename: filename.to_string(),
            content: content.to_string(),
        }
    }

    fn provenance(format: EditorExportFormat, base: &str) -> String {
        format!("{{\"schema_version\":\"maestro.export-provenance.v1\",\"format\":\"{}\",\"document\":{{\"filename\":\"{base}{}\"}},\"evidence\":[]}}\n", format.directory(), format.content_suffix())
    }

    #[test]
    fn persists_exact_editor_exports_in_portable_subtype_directories() {
        for (format, filename, content) in [
            (
                EditorExportFormat::Markdown,
                "portable-storage-fixture.md",
                "# Revisão\r\n\r\nTexto.\n",
            ),
            (
                EditorExportFormat::Html,
                "portable-storage-fixture.mainsite.html",
                "<p>Revisão &amp; conteúdo.</p>\n",
            ),
        ] {
            let input = request(format, filename, content);
            let expected = data_dir()
                .join("exports")
                .join(format.directory())
                .join(filename);
            assert_eq!(validated_export_path(&input).unwrap(), expected);
            let saved = persist_editor_export_inner(input).unwrap();
            assert_eq!(
                saved.relative_path,
                format!("exports/{}/{filename}", format.directory())
            );
            assert_eq!(saved.bytes, content.len());
            assert_eq!(std::fs::read(&expected).unwrap(), content.as_bytes());
            // Re-export replaces the same artifact atomically in its subtype.
            persist_editor_export_inner(request(format, filename, "Updated final content"))
                .unwrap();
            assert_eq!(
                std::fs::read_to_string(expected).unwrap(),
                "Updated final content"
            );
        }
        for format in [
            EditorExportFormat::Markdown,
            EditorExportFormat::Html,
            EditorExportFormat::Pdf,
        ] {
            let content = provenance(format, "portable-storage-fixture");
            let input = request(format, "portable-storage-fixture.provenance.json", &content);
            let path = validated_export_path(&input).unwrap();
            assert_eq!(
                path,
                data_dir()
                    .join("exports")
                    .join(format.directory())
                    .join("portable-storage-fixture.provenance.json")
            );
            persist_editor_export_inner(input).unwrap();
            assert_eq!(std::fs::read(path).unwrap(), content.as_bytes());
        }
    }

    #[test]
    fn rejects_export_traversal_wrong_extensions_and_windows_device_names() {
        for filename in [
            "../outside.md",
            "C:\\Downloads\\outside.md",
            "outside.html",
            "outside.exe",
            "con.md",
            "com1.md",
            "lpt9.md",
            "outside/child.md",
            "-outside.md",
            "outside-.md",
        ] {
            assert!(
                validated_export_path(&request(EditorExportFormat::Markdown, filename, "article"))
                    .is_err(),
                "accepted {filename}"
            );
        }
        assert!(validated_export_path(&request(
            EditorExportFormat::Pdf,
            "outside.pdf",
            "not a PDF"
        ))
        .is_err());
        assert!(validated_export_path(&request(
            EditorExportFormat::Html,
            "empty.mainsite.html",
            ""
        ))
        .is_err());
        assert!(validated_export_path(&request(
            EditorExportFormat::Markdown,
            "oversized.md",
            &"x".repeat(MAX_EXPORT_BYTES + 1)
        ))
        .is_err());
        assert!(serde_json::from_str::<EditorExportRequest>(
            r#"{"format":"exe","filename":"x.exe","content":"x"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<EditorExportRequest>(
            r#"{"format":"markdown","filename":"x.md","content":"x","path":"C:\\Downloads"}"#
        )
        .is_err());
    }

    #[test]
    fn surfaces_real_disk_write_failure_without_reporting_an_export() {
        let input = request(
            EditorExportFormat::Html,
            "portable-storage-disk-failure.mainsite.html",
            "<p>Preserved</p>",
        );
        let path = validated_export_path(&input).unwrap();
        std::fs::create_dir_all(&path).unwrap();
        let result = persist_editor_export_inner(input);
        assert!(result.is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        std::fs::remove_dir(path).unwrap();
    }

    #[test]
    fn rejects_provenance_drift_before_writing_any_file() {
        for content in [
            "invalid JSON".to_string(),
            provenance(EditorExportFormat::Html, "different-title"),
            provenance(
                EditorExportFormat::Markdown,
                "portable-storage-drift-fixture",
            ),
            provenance(EditorExportFormat::Html, "portable-storage-drift-fixture")
                .replace("maestro.export-provenance.v1", "untrusted.v1"),
        ] {
            let input = request(
                EditorExportFormat::Html,
                "portable-storage-drift-fixture.provenance.json",
                &content,
            );
            assert!(persist_editor_export_inner(input).is_err());
        }
        assert!(!data_dir()
            .join("exports/html/portable-storage-drift-fixture.provenance.json")
            .exists());
    }
}
