//! Source-bound declarations of protocol coverage. A model declaration is not
//! evidence of cognition. All selected peers must pass this private preflight
//! before the existing drafting/review loop can run.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::app_paths::{checked_data_child_path, sanitize_path_segment, sessions_dir};
use crate::editorial_io::{read_text_file, write_text_file};
use crate::session_artifacts::circular_draft_sha256;
use crate::{EditorialAgentResult, EditorialSessionRequest};

const SECTION_LINES: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtocolLineRange {
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtocolSection {
    pub(crate) section_id: String,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtocolAckSource {
    pub(crate) protocol_name: String,
    pub(crate) protocol_hash: String,
    pub(crate) byte_count: usize,
    pub(crate) line_count_expected: usize,
    pub(crate) sections: Vec<ProtocolSection>,
}

impl ProtocolAckSource {
    pub(crate) fn from_request(request: &EditorialSessionRequest) -> Self {
        let line_count_expected = request.protocol_text.lines().count();
        let sections = (1..=line_count_expected)
            .step_by(SECTION_LINES)
            .map(|start_line| {
                let end_line = (start_line + SECTION_LINES - 1).min(line_count_expected);
                ProtocolSection {
                    section_id: format!("L{start_line:06}-L{end_line:06}"),
                    start_line,
                    end_line,
                }
            })
            .collect();
        Self {
            protocol_name: request.protocol_name.clone(),
            protocol_hash: circular_draft_sha256(&request.protocol_text),
            byte_count: request.protocol_text.len(),
            line_count_expected,
            sections,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProtocolAcknowledgement {
    pub(crate) protocol_name: String,
    pub(crate) protocol_hash: String,
    pub(crate) line_count_expected: usize,
    pub(crate) line_count_acknowledged: usize,
    pub(crate) read_mode: String,
    pub(crate) acknowledged_sections: Vec<ProtocolSection>,
    pub(crate) missing_ranges: Vec<ProtocolLineRange>,
    pub(crate) status: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProtocolAckRecord {
    schema_version: u8,
    run_id: String,
    invocation_id: String,
    agent_key: String,
    agent_name: String,
    captured_at: String,
    source: ProtocolAckSource,
    acknowledgement: Option<ProtocolAcknowledgement>,
    admitted: bool,
    validation_error: Option<String>,
    native_status: String,
    native_tone: String,
    artifact_path: String,
}

pub(crate) fn build_protocol_ack_prompt(
    request: &EditorialSessionRequest,
    source: &ProtocolAckSource,
) -> String {
    let manifest = serde_json::to_string_pretty(source).expect("protocol source is serializable");
    format!(
        "Read the complete pinned editorial protocol below before acknowledging coverage. This is a protocol-reading declaration, not drafting or editorial approval. Do not generate an article or MAESTRO_STATUS marker.\n\nReturn exactly one JSON object without fences, prose, duplicate or extra fields. Required fields: protocol_name, protocol_hash, line_count_expected, line_count_acknowledged, read_mode, acknowledged_sections, missing_ranges, status. Copy the exact source identity and native SHA-256 below. Line numbers are one-based physical lines using the supplied manifest; a final newline does not add an extra empty line. Sections are deterministic contiguous groups of at most 128 lines, including headings, blank lines and code. After declaring full coverage, acknowledged_sections must contain every manifest section exactly once and in order, line_count_acknowledged must equal line_count_expected, read_mode must be full_line_by_line, missing_ranges must be [], and status must be ACKNOWLEDGED. Each section has section_id, start_line and end_line. If coverage is incomplete, report the sections actually covered and missing_ranges (start_line and end_line), use status INCOMPLETE and read_mode partial; never invent complete coverage to pass a gate. This declaration cannot prove every line was cognitively processed.\n\n## Native pinned protocol source\n\n{manifest}\n\n## Complete pinned editorial protocol\n\n{}",
        request.protocol_text
    )
}

pub(crate) fn validate_protocol_acknowledgement(
    acknowledgement: &ProtocolAcknowledgement,
    source: &ProtocolAckSource,
) -> Result<(), String> {
    if acknowledgement.protocol_name != source.protocol_name {
        return Err("protocol identity does not match the pinned source".to_string());
    }
    if acknowledgement.protocol_hash != source.protocol_hash {
        return Err("protocol SHA-256 does not match the pinned bytes".to_string());
    }
    if acknowledgement.line_count_expected != source.line_count_expected
        || acknowledgement.line_count_acknowledged != source.line_count_expected
    {
        return Err(
            "declared line coverage is incomplete or differs from the pinned source".to_string(),
        );
    }
    if acknowledgement.read_mode != "full_line_by_line" || acknowledgement.status != "ACKNOWLEDGED"
    {
        return Err("peer did not declare full line-by-line coverage".to_string());
    }
    if !acknowledgement.missing_ranges.is_empty() {
        return Err("peer reported missing protocol line ranges".to_string());
    }
    if acknowledgement.acknowledged_sections != source.sections {
        return Err("declared sections do not cover the exact pinned line ranges".to_string());
    }
    Ok(())
}

pub(crate) fn protocol_ack_attempt_path(
    session_dir: &Path,
    invocation_id: &str,
    agent_key: &str,
) -> Result<PathBuf, String> {
    // Invocation IDs are generated by the existing session cost scope. Hashing
    // that identity yields a private filename without exposing local paths.
    let invocation_hash = circular_draft_sha256(invocation_id);
    let path =
        checked_data_child_path(&session_dir.join("protocol-acknowledgements").join(format!(
            "ack-{}-{}.md",
            &invocation_hash[..24],
            sanitize_path_segment(agent_key, 40)
        )))?;
    if path.exists() || path.with_extension("json").exists() {
        return Err("protocol acknowledgement attempt already exists".to_string());
    }
    Ok(path)
}

pub(crate) fn record_protocol_acknowledgement(
    request: &EditorialSessionRequest,
    source: &ProtocolAckSource,
    invocation_id: &str,
    agent_key: &str,
    result: &mut EditorialAgentResult,
) -> Result<bool, String> {
    let artifact_path = checked_data_child_path(Path::new(&result.output_path))?;
    let artifact = read_text_file(&artifact_path)?;
    let mut acknowledgement = None;
    let validation = if result.tone != "ok" || result.exit_code != Some(0) {
        Err("native invocation failed; no protocol coverage is admitted".to_string())
    } else {
        extract_native_ack_stdout(&artifact)
            .and_then(|stdout| {
                serde_json::from_str::<ProtocolAcknowledgement>(stdout.trim()).map_err(|error| {
                    format!("acknowledgement must be one strict JSON object: {error}")
                })
            })
            .and_then(|parsed| {
                let validation = validate_protocol_acknowledgement(&parsed, source);
                acknowledgement = Some(parsed);
                validation
            })
    };
    let admitted = validation.is_ok();
    let record = ProtocolAckRecord {
        schema_version: 1,
        run_id: request.run_id.clone(),
        invocation_id: invocation_id.to_string(),
        agent_key: agent_key.to_string(),
        agent_name: result.name.clone(),
        captured_at: Utc::now().to_rfc3339(),
        source: source.clone(),
        acknowledgement,
        admitted,
        validation_error: validation.err(),
        native_status: result.status.clone(),
        native_tone: result.tone.clone(),
        artifact_path: result.output_path.clone(),
    };
    let json_path = artifact_path.with_extension("json");
    if json_path.exists() {
        return Err("protocol acknowledgement record already exists".to_string());
    }
    write_text_file(
        &json_path,
        &serde_json::to_string_pretty(&record)
            .map_err(|error| format!("failed to serialize acknowledgement: {error}"))?,
    )?;
    // Keep native raw artifacts unchanged. Only the in-memory product outcome
    // becomes a coverage declaration; this is never an editorial READY vote.
    if result.tone == "ok" && result.exit_code == Some(0) {
        result.status = if admitted {
            "PROTOCOL_ACKNOWLEDGED"
        } else {
            "PROTOCOL_ACK_INVALID"
        }
        .to_string();
        result.tone = if admitted { "ok" } else { "blocked" }.to_string();
    }
    Ok(admitted)
}

fn extract_native_ack_stdout(artifact: &str) -> Result<&str, String> {
    let (header, rest) = artifact
        .split_once("\n## Stdout\n\n```text\n")
        .ok_or_else(|| "native acknowledgement artifact has no stdout block".to_string())?;
    // Every successful native writer records the exact stdout character count.
    // Use that boundary rather than provider-controlled Markdown delimiters.
    let mut lengths = header
        .lines()
        .filter_map(|line| line.strip_prefix("- Stdout chars: `"));
    let length = lengths
        .next()
        .and_then(|value| value.strip_suffix('`'))
        .and_then(|value| value.parse::<usize>().ok())
        .ok_or_else(|| "native acknowledgement artifact has no valid stdout length".to_string())?;
    if lengths.next().is_some() {
        return Err("native acknowledgement artifact has duplicate stdout lengths".to_string());
    }
    let end = rest
        .char_indices()
        .nth(length)
        .map(|(index, _)| index)
        .ok_or_else(|| {
            "native acknowledgement artifact has an invalid stdout length".to_string()
        })?;
    if !rest[end..].starts_with("\n```\n\n## Stderr\n\n```text\n") || !rest.ends_with("\n```\n") {
        return Err(
            "native acknowledgement artifact has an inconsistent stdout boundary".to_string(),
        );
    }
    Ok(&rest[..end])
}

pub(crate) fn build_protocol_ack_minutes(
    request: &EditorialSessionRequest,
    run_id: &str,
) -> String {
    let source = ProtocolAckSource::from_request(request);
    let mut text = format!(
        "\n## Reconhecimento declarado do protocolo\n\n- Identidade ativa: `{}`\n- SHA-256 dos bytes fornecidos: `{}`\n- Linhas esperadas: `{}`\n\nO reconhecimento abaixo registra declaracoes dos peers validadas mecanicamente contra a fonte. Nao comprova cognicao de cada linha e nao atribui percentuais de leitura. Cada inicio/retomada exige novas declaracoes de todos os peers selecionados antes de qualquer rodada.\n",
        crate::sanitize_text(&source.protocol_name, 200), source.protocol_hash, source.line_count_expected
    );
    let dir = sessions_dir()
        .join(sanitize_path_segment(run_id, 120))
        .join("protocol-acknowledgements");
    let (records, invalid_records) = read_protocol_ack_records(&dir);
    for name in invalid_records {
        text.push_str(&format!("\nRegistro historico de reconhecimento ilegivel/invalido: `{name}`. O arquivo permanece preservado e nao admite esta chamada.\n"));
    }
    if records.is_empty() {
        text.push_str("\nNenhum reconhecimento estruturado registrado nesta sessao.\n");
    }
    for record in records {
        let current_source = record.source == source;
        text.push_str(&format!(
            "\n### {} — {}\n\n- Fonte corresponde ao protocolo ativo: `{current_source}`\n- Admitido nesta tentativa: `{}`\n- Artefato bruto privado: `{}`\n\n```json\n{}\n```\n",
            record.agent_name, record.captured_at, record.admitted, record.artifact_path,
            serde_json::to_string_pretty(&record).unwrap_or_default()
        ));
    }
    text
}

fn read_protocol_ack_records(dir: &Path) -> (Vec<ProtocolAckRecord>, Vec<String>) {
    let Ok(dir) = checked_data_child_path(dir) else {
        return (Vec::new(), Vec::new());
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut records = Vec::new();
    let mut invalid_records = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.starts_with("ack-")
            || path.extension().and_then(|value| value.to_str()) != Some("json")
        {
            continue;
        }
        match read_text_file(&path).and_then(|text| {
            serde_json::from_str::<ProtocolAckRecord>(&text).map_err(|error| error.to_string())
        }) {
            Ok(record) if record.schema_version == 1 => records.push(record),
            _ => invalid_records.push(crate::sanitize_text(name, 200)),
        }
    }
    records.sort_by(|left, right| {
        left.captured_at
            .cmp(&right.captured_at)
            .then_with(|| left.agent_key.cmp(&right.agent_key))
    });
    (records, invalid_records)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn test_ack(source: &ProtocolAckSource) -> ProtocolAcknowledgement {
        ProtocolAcknowledgement {
            protocol_name: source.protocol_name.clone(),
            protocol_hash: source.protocol_hash.clone(),
            line_count_expected: source.line_count_expected,
            line_count_acknowledged: source.line_count_expected,
            read_mode: "full_line_by_line".to_string(),
            acknowledged_sections: source.sections.clone(),
            missing_ranges: vec![],
            status: "ACKNOWLEDGED".to_string(),
        }
    }

    fn source() -> ProtocolAckSource {
        ProtocolAckSource {
            protocol_name: "protocolo-integral.md".to_string(),
            protocol_hash: "a".repeat(64),
            byte_count: 1_024,
            line_count_expected: 256,
            sections: vec![
                ProtocolSection {
                    section_id: "L000001-L000128".to_string(),
                    start_line: 1,
                    end_line: 128,
                },
                ProtocolSection {
                    section_id: "L000129-L000256".to_string(),
                    start_line: 129,
                    end_line: 256,
                },
            ],
        }
    }

    #[test]
    fn protocol_ack_rejects_wrong_identity_hash_and_incomplete_coverage() {
        let source = source();
        let good = test_ack(&source);
        validate_protocol_acknowledgement(&good, &source).unwrap();
        let mut bad = good.clone();
        bad.protocol_name.push_str("-other");
        assert!(validate_protocol_acknowledgement(&bad, &source)
            .unwrap_err()
            .contains("identity"));
        let mut bad = good.clone();
        bad.protocol_hash = "b".repeat(64);
        assert!(validate_protocol_acknowledgement(&bad, &source)
            .unwrap_err()
            .contains("SHA-256"));
        for change in 0..7 {
            let mut bad = good.clone();
            match change {
                0 => bad.line_count_expected -= 1,
                1 => bad.line_count_acknowledged -= 1,
                2 => bad.read_mode = "partial".to_string(),
                3 => bad.status = "INCOMPLETE".to_string(),
                4 => bad.missing_ranges.push(ProtocolLineRange {
                    start_line: 2,
                    end_line: 3,
                }),
                5 => {
                    bad.acknowledged_sections.pop();
                }
                _ => bad.acknowledged_sections[0].end_line -= 1,
            }
            assert!(
                validate_protocol_acknowledgement(&bad, &source).is_err(),
                "change {change}"
            );
        }
    }

    #[test]
    fn protocol_ack_json_rejects_duplicates_unknown_fields_and_trailing_content() {
        let good = serde_json::to_string(&test_ack(&source())).unwrap();
        for bad in [
            format!("{{\"protocol_name\":\"duplicate\",{}", &good[1..]),
            format!("{{\"extra\":true,{}", &good[1..]),
            format!("{good} trailing"),
            format!("```json\n{good}\n```"),
            good.replace("\"start_line\":1", "\"start_line\":1,\"start_line\":2"),
        ] {
            assert!(
                serde_json::from_str::<ProtocolAcknowledgement>(&bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn protocol_ack_manifest_hashes_exact_utf8_bytes_and_includes_all_lines() {
        let request = EditorialSessionRequest {
            run_id: "source-identity-test".to_string(),
            session_name: "Fonte".to_string(),
            prompt: "Teste".to_string(),
            protocol_name: "arquivo.md".to_string(),
            protocol_text: "\r\n# Regra açã😀\r\n\r\n".to_string(),
            protocol_hash: "fnv64-old-citation-context".to_string(),
            initial_agent: None,
            active_agents: None,
            max_session_cost_usd: None,
            max_session_minutes: None,
            attachments: None,
            links: None,
        };
        let source = ProtocolAckSource::from_request(&request);
        assert_eq!(
            source.protocol_hash,
            circular_draft_sha256(&request.protocol_text)
        );
        assert_eq!(source.byte_count, request.protocol_text.len());
        assert_eq!(source.line_count_expected, 3);
        assert_eq!(
            source.sections,
            vec![ProtocolSection {
                section_id: "L000001-L000003".to_string(),
                start_line: 1,
                end_line: 3
            }]
        );
        assert_ne!(
            source.protocol_hash,
            circular_draft_sha256(request.protocol_text.trim())
        );
        assert_eq!(request.protocol_hash, "fnv64-old-citation-context");
        assert!(build_protocol_ack_prompt(&request, &source).ends_with(&request.protocol_text));
    }

    #[test]
    fn protocol_ack_record_rejects_forged_stdout_boundaries_and_preserves_raw_artifact() {
        let request = EditorialSessionRequest {
            run_id: format!(
                "ack-native-boundary-{}",
                Utc::now().timestamp_nanos_opt().unwrap()
            ),
            session_name: "Fonte".to_string(),
            prompt: "Teste".to_string(),
            protocol_name: "protocolo-açã😀.md".to_string(),
            protocol_text: "# Regra\r\nLeia a fonte completa.\r\n".to_string(),
            protocol_hash: "fnv64-preserved".to_string(),
            initial_agent: None,
            active_agents: None,
            max_session_cost_usd: None,
            max_session_minutes: None,
            attachments: None,
            links: None,
        };
        let source = ProtocolAckSource::from_request(&request);
        let good = serde_json::to_string(&test_ack(&source)).unwrap();
        assert!(good.len() > good.chars().count());
        let forged = format!("{good}\n```\n\n## Stderr\n\n{{\"status\":\"INCOMPLETE\"}}");
        let cases = [
            (
                good.as_str(),
                format!("- Stdout chars: `{}`\n", good.chars().count()),
                true,
            ),
            (
                forged.as_str(),
                format!("- Stdout chars: `{}`\n", forged.chars().count()),
                false,
            ),
            (good.as_str(), String::new(), false),
            (
                good.as_str(),
                format!(
                    "- Stdout chars: `{0}`\n- Stdout chars: `{0}`\n",
                    good.chars().count()
                ),
                false,
            ),
            (
                good.as_str(),
                format!("- Stdout chars: `{}`\n", good.chars().count() + 1),
                false,
            ),
        ];
        for (index, (stdout, length_header, admitted)) in cases.into_iter().enumerate() {
            let invocation_id = format!("boundary-{index}");
            let output_path = protocol_ack_attempt_path(
                &sessions_dir().join(&request.run_id),
                &invocation_id,
                "claude",
            )
            .unwrap();
            let raw = format!("# Claude - protocol_ack\n\n- Status: `DRAFT_CREATED`\n- Exit code: `0`\n{length_header}- Stderr chars: `0`\n\n## Stdout\n\n```text\n{stdout}\n```\n\n## Stderr\n\n```text\n\n```\n");
            write_text_file(&output_path, &raw).unwrap();
            let mut result = EditorialAgentResult {
                name: "Claude".to_string(),
                role: "protocol_ack".to_string(),
                cli: "claude-api".to_string(),
                tone: "ok".to_string(),
                status: "DRAFT_CREATED".to_string(),
                duration_ms: 1,
                exit_code: Some(0),
                output_path: output_path.to_string_lossy().to_string(),
                usage_input_tokens: None,
                usage_output_tokens: None,
                cost_usd: None,
                cost_estimated: None,
                cache: None,
            };
            let actual = record_protocol_acknowledgement(
                &request,
                &source,
                &invocation_id,
                "claude",
                &mut result,
            )
            .unwrap();
            assert_eq!(actual, admitted, "native stdout must remain complete");
            assert_eq!(read_text_file(&output_path).unwrap(), raw);
            let record: ProtocolAckRecord =
                serde_json::from_str(&read_text_file(&output_path.with_extension("json")).unwrap())
                    .unwrap();
            assert_eq!(record.admitted, admitted);
            assert_eq!(record.native_status, "DRAFT_CREATED");
            assert_eq!(
                result.status,
                if admitted {
                    "PROTOCOL_ACKNOWLEDGED"
                } else {
                    "PROTOCOL_ACK_INVALID"
                }
            );
        }
    }
}
