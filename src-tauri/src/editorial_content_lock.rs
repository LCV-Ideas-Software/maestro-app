use std::collections::BTreeMap;

use regex::Regex;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorialContentBlock {
    pub(crate) id: String,
    pub(crate) kind: &'static str,
    text: String,
    normalized_hash: String,
    chars: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ChangedBlockDeclaration {
    has_protocol_basis: bool,
    allows_block_count_growth: bool,
    allows_reorder: bool,
}

pub(crate) fn segment_editorial_blocks(text: &str) -> Vec<EditorialContentBlock> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split("\n\n")
        .filter_map(|raw_block| {
            let trimmed = raw_block.trim();
            if trimmed.is_empty() {
                return None;
            }
            Some(trimmed.to_string())
        })
        .enumerate()
        .map(|(index, block)| {
            let normalized = normalize_block_text(&block);
            EditorialContentBlock {
                id: format!("B{:04}", index + 1),
                kind: classify_block_kind(&block),
                chars: block.chars().count(),
                normalized_hash: sha256_hex(&normalized),
                text: block,
            }
        })
        .collect()
}

pub(crate) fn format_block_manifest_for_prompt(text: &str) -> String {
    let blocks = segment_editorial_blocks(text);
    if blocks.is_empty() {
        return "No editorial content blocks were detected.".to_string();
    }

    let mut lines = vec![
        "| block_id | kind | chars | sha256_12 | locked_by_default | excerpt |".to_string(),
        "|---|---:|---:|---|---|---|".to_string(),
    ];
    for block in blocks {
        lines.push(format!(
            "| {} | {} | {} | {} | yes | {} |",
            block.id,
            block.kind,
            block.chars,
            &block.normalized_hash[..12],
            markdown_table_excerpt(&block.text)
        ));
    }
    lines.join("\n")
}

pub(crate) fn validate_revision_content_lock(
    before: &str,
    after: &str,
    report: &str,
) -> Result<(), String> {
    let before_blocks = segment_editorial_blocks(before);
    let after_blocks = segment_editorial_blocks(after);
    let changed_ids = changed_received_block_ids(&before_blocks, &after_blocks);
    let Some(changed_section) = extract_changed_blocks_section(report) else {
        if changed_ids.is_empty() && after_blocks.len() <= before_blocks.len() {
            return Ok(());
        }
        return Err(format!(
            "approved-content lock violation: revised custody changed received blocks {} but maestro_revision_report has no changed_blocks section with block IDs",
            changed_ids.join(", ")
        ));
    };
    let declarations = extract_changed_block_declarations(changed_section);

    let undeclared = changed_ids
        .iter()
        .filter(|id| !declarations.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    if !undeclared.is_empty() {
        return Err(format!(
            "approved-content lock violation: changed received blocks {} without matching changed_blocks declaration",
            undeclared.join(", ")
        ));
    }

    let missing_protocol_basis = changed_ids
        .iter()
        .filter(|id| {
            declarations
                .get(*id)
                .map(|declaration| !declaration.has_protocol_basis)
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !missing_protocol_basis.is_empty() {
        return Err(format!(
            "approved-content lock violation: changed_blocks entries for {} must include protocol_basis",
            missing_protocol_basis.join(", ")
        ));
    }

    if after_blocks.len() > before_blocks.len()
        && !declarations
            .values()
            .any(|declaration| declaration.allows_block_count_growth)
    {
        return Err(
            "approved-content lock violation: revised custody added new blocks without declaring change_type split/addition in changed_blocks"
                .to_string(),
        );
    }

    if received_block_order_changed(&before_blocks, &after_blocks)
        && !declarations
            .values()
            .any(|declaration| declaration.allows_reorder)
    {
        return Err(
            "approved-content lock violation: revised custody reordered received blocks without declaring change_type reorder in changed_blocks"
                .to_string(),
        );
    }

    Ok(())
}

fn normalize_block_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn classify_block_kind(text: &str) -> &'static str {
    let trimmed = text.trim_start();
    if trimmed.starts_with('#') {
        "heading"
    } else if trimmed.starts_with('>') {
        "quote"
    } else if trimmed.lines().all(|line| {
        let line = line.trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line
                .chars()
                .next()
                .map(|ch| ch.is_ascii_digit())
                .unwrap_or(false)
    }) {
        "list"
    } else if trimmed.lines().filter(|line| line.contains('|')).count() >= 2 {
        "table"
    } else {
        "paragraph"
    }
}

fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn markdown_table_excerpt(text: &str) -> String {
    let compact = normalize_block_text(text)
        .replace('|', "\\|")
        .replace('\n', " ");
    let mut excerpt = compact.chars().take(96).collect::<String>();
    if compact.chars().count() > 96 {
        excerpt.push_str("...");
    }
    excerpt
}

fn changed_received_block_ids(
    before_blocks: &[EditorialContentBlock],
    after_blocks: &[EditorialContentBlock],
) -> Vec<String> {
    let mut after_hash_counts = BTreeMap::<&str, usize>::new();
    for after in after_blocks {
        *after_hash_counts
            .entry(after.normalized_hash.as_str())
            .or_insert(0) += 1;
    }

    before_blocks
        .iter()
        .filter_map(|before| {
            let count = after_hash_counts
                .entry(before.normalized_hash.as_str())
                .or_insert(0);
            if *count == 0 {
                Some(before.id.clone())
            } else {
                *count -= 1;
                None
            }
        })
        .collect()
}

fn received_block_order_changed(
    before_blocks: &[EditorialContentBlock],
    after_blocks: &[EditorialContentBlock],
) -> bool {
    let mut before_counts = BTreeMap::<String, usize>::new();
    let mut after_counts = BTreeMap::<String, usize>::new();
    for block in before_blocks {
        *before_counts
            .entry(block.normalized_hash.clone())
            .or_insert(0) += 1;
    }
    for block in after_blocks {
        *after_counts
            .entry(block.normalized_hash.clone())
            .or_insert(0) += 1;
    }

    let mut common_counts = BTreeMap::<String, usize>::new();
    for (hash, before_count) in before_counts {
        if let Some(after_count) = after_counts.get(&hash) {
            common_counts.insert(hash, before_count.min(*after_count));
        }
    }
    if common_counts.values().sum::<usize>() <= 1 {
        return false;
    }

    common_block_hash_sequence(before_blocks, &common_counts)
        != common_block_hash_sequence(after_blocks, &common_counts)
}

fn common_block_hash_sequence(
    blocks: &[EditorialContentBlock],
    common_counts: &BTreeMap<String, usize>,
) -> Vec<String> {
    let mut remaining = common_counts.clone();
    let mut sequence = Vec::new();
    for block in blocks {
        if let Some(count) = remaining.get_mut(&block.normalized_hash) {
            if *count > 0 {
                sequence.push(block.normalized_hash.clone());
                *count -= 1;
            }
        }
    }
    sequence
}

fn extract_changed_blocks_section(report: &str) -> Option<&str> {
    let lower = report.to_ascii_lowercase();
    let start = find_first_key(
        &lower,
        &[
            "\"changed_blocks\"",
            "changed_blocks",
            "\"changes\"",
            "changes",
        ],
    )?;
    let relative_end = find_first_key(
        &lower[start + 1..],
        &[
            "\"operator_evidence_required\"",
            "operator_evidence_required",
            "\"out_of_scope\"",
            "out_of_scope",
            "\"quality_preservation\"",
            "quality_preservation",
            "\"unchanged_approved_blocks\"",
            "unchanged_approved_blocks",
            "\"custody\"",
            "custody",
        ],
    );
    let end = relative_end
        .map(|offset| start + 1 + offset)
        .unwrap_or(report.len());
    report.get(start..end)
}

fn find_first_key(haystack: &str, keys: &[&str]) -> Option<usize> {
    keys.iter().filter_map(|key| haystack.find(key)).min()
}

fn extract_changed_block_declarations(section: &str) -> BTreeMap<String, ChangedBlockDeclaration> {
    let mut declarations = BTreeMap::new();
    for fragment in changed_block_entry_fragments(section) {
        let Some(block_id) = extract_block_id_field(&fragment) else {
            continue;
        };
        let declaration = ChangedBlockDeclaration {
            has_protocol_basis: fragment_has_nonempty_protocol_basis(&fragment),
            allows_block_count_growth: fragment_declares_block_count_growth(&fragment),
            allows_reorder: fragment_declares_reorder(&fragment),
        };
        declarations
            .entry(block_id)
            .and_modify(|existing: &mut ChangedBlockDeclaration| {
                existing.has_protocol_basis |= declaration.has_protocol_basis;
                existing.allows_block_count_growth |= declaration.allows_block_count_growth;
                existing.allows_reorder |= declaration.allows_reorder;
            })
            .or_insert(declaration);
    }
    declarations
}

fn changed_block_entry_fragments(section: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    for (index, character) in section.char_indices() {
        if character == '{' {
            if depth == 0 {
                start = Some(index);
            }
            depth += 1;
        } else if character == '}' && depth > 0 {
            depth -= 1;
            if depth == 0 {
                if let Some(start_index) = start.take() {
                    fragments.push(section[start_index..=index].to_string());
                }
            }
        }
    }

    if fragments.is_empty() {
        fragments.extend(
            section
                .lines()
                .filter(|line| line.to_ascii_lowercase().contains("block_id"))
                .map(|line| line.to_string()),
        );
    }
    fragments
}

fn extract_block_id_field(fragment: &str) -> Option<String> {
    Regex::new(r#"(?is)["']?block_id["']?\s*[:=]\s*["']?(B\d{4})\b"#)
        .expect("valid block_id field regex")
        .captures(fragment)
        .and_then(|captures| captures.get(1))
        .map(|matched| matched.as_str().to_string())
}

fn fragment_has_nonempty_protocol_basis(fragment: &str) -> bool {
    Regex::new(r#"(?is)["']?protocol_basis["']?\s*[:=]\s*(?:"([^"]+)"|'([^']+)'|([^\s,}\]]+))"#)
        .expect("valid protocol_basis field regex")
        .captures(fragment)
        .and_then(|captures| {
            captures
                .get(1)
                .or_else(|| captures.get(2))
                .or_else(|| captures.get(3))
        })
        .map(|matched| {
            let value = matched.as_str().trim();
            !value.is_empty() && value != "null" && value != "[]" && value != "{}"
        })
        .unwrap_or(false)
}

fn fragment_declares_block_count_growth(fragment: &str) -> bool {
    let lower = fragment.to_ascii_lowercase();
    lower.contains("change_type")
        && (lower.contains("split")
            || lower.contains("addition")
            || lower.contains("added")
            || lower.contains("new_block")
            || lower.contains("new block"))
}

fn fragment_declares_reorder(fragment: &str) -> bool {
    let lower = fragment.to_ascii_lowercase();
    lower.contains("change_type")
        && (lower.contains("reorder")
            || lower.contains("reordered")
            || lower.contains("move")
            || lower.contains("moved"))
}

#[cfg(test)]
mod tests {
    use super::{
        format_block_manifest_for_prompt, segment_editorial_blocks, validate_revision_content_lock,
    };

    #[test]
    fn changed_block_without_changed_blocks_declaration_is_rejected() {
        let before =
            "# Titulo\n\nParagrafo aprovado e denso.\n\nReferencia pendente [EVIDENCIA_PENDENTE].";
        let after = "# Titulo\n\nParagrafo encurtado.\n\nReferencia removida.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0003", "protocol_basis": "bibliographic integrity"}
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("B0002"), "{error}");
    }

    #[test]
    fn declared_changed_block_with_protocol_basis_is_allowed() {
        let before =
            "# Titulo\n\nParagrafo aprovado e denso.\n\nReferencia pendente [EVIDENCIA_PENDENTE].";
        let after = "# Titulo\n\nParagrafo aprovado e denso.\n\nReferencia removida.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0003", "protocol_basis": "bibliographic integrity"}
          ],
          "custody": "revised"
        }"#;

        validate_revision_content_lock(before, after, report).unwrap();
    }

    #[test]
    fn changed_block_declaration_requires_protocol_basis() {
        let before = "# Titulo\n\nParagrafo aprovado.\n\nReferencia pendente [EVIDENCIA_PENDENTE].";
        let after = "# Titulo\n\nParagrafo aprovado.\n\nReferencia removida.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0003", "reason": "removed reference"}
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("protocol_basis"), "{error}");
    }

    #[test]
    fn each_changed_block_requires_its_own_protocol_basis() {
        let before =
            "# Titulo\n\nParagrafo aprovado e denso.\n\nReferencia pendente [EVIDENCIA_PENDENTE].";
        let after = "# Titulo\n\nParagrafo encurtado.\n\nReferencia removida.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "reason": "shortened"},
            {"block_id": "B0003", "protocol_basis": "bibliographic integrity"}
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("B0002"), "{error}");
        assert!(error.contains("protocol_basis"), "{error}");
    }

    #[test]
    fn block_id_mentioned_in_reason_does_not_authorize_that_block() {
        let before =
            "# Titulo\n\nParagrafo aprovado e denso.\n\nReferencia pendente [EVIDENCIA_PENDENTE].";
        let after = "# Titulo\n\nParagrafo encurtado.\n\nReferencia removida.";
        let report = r#"{
          "changed_blocks": [
            {
              "block_id": "B0003",
              "reason": "removed reference and mentioned B0002 only as surrounding context",
              "protocol_basis": "bibliographic integrity"
            }
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("B0002"), "{error}");
        assert!(
            error.contains("without matching changed_blocks declaration"),
            "{error}"
        );
    }

    #[test]
    fn stealth_addition_is_rejected_even_when_another_block_changed() {
        let before = "# Titulo\n\nParagrafo aprovado.\n\nReferencia pendente [EVIDENCIA_PENDENTE].";
        let after =
            "# Titulo\n\nParagrafo aprovado.\n\nReferencia removida.\n\nNovo argumento indevido.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0003", "protocol_basis": "bibliographic integrity"}
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("added new blocks"), "{error}");
    }

    #[test]
    fn declared_addition_does_not_make_later_unchanged_blocks_look_changed() {
        let before = "# Titulo\n\nParagrafo aprovado.\n\nConclusao aprovada.";
        let after =
            "# Titulo\n\nNovo contexto necessario.\n\nParagrafo aprovado.\n\nConclusao aprovada.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "change_type": "addition", "protocol_basis": "required context"}
          ],
          "custody": "revised"
        }"#;

        validate_revision_content_lock(before, after, report).unwrap();
    }

    #[test]
    fn silent_reorder_of_received_blocks_is_rejected() {
        let before = "# Titulo\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.";
        let after = "# Titulo\n\nSegundo bloco aprovado.\n\nPrimeiro bloco aprovado.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "protocol_basis": "style preference"}
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("reordered received blocks"), "{error}");
    }

    #[test]
    fn reorder_with_concurrent_declared_edit_is_rejected_without_reorder_declaration() {
        let before = "# Titulo\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.\n\nTerceiro bloco aprovado.";
        let after = "# Titulo\n\nTerceiro bloco aprovado.\n\nPrimeiro bloco editado.\n\nSegundo bloco aprovado.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "protocol_basis": "editorial correction"}
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("reordered received blocks"), "{error}");
    }

    #[test]
    fn declared_changed_received_block_may_be_split_without_extra_addition_keyword() {
        let before =
            "# Titulo\n\nParagrafo longo com uma referencia pendente [EVIDENCIA_PENDENTE].";
        let after =
            "# Titulo\n\nParagrafo longo sem a referencia pendente.\n\nNota editorial preservada.";
        let report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "change_type": "split", "protocol_basis": "bibliographic integrity"}
          ],
          "custody": "revised"
        }"#;

        validate_revision_content_lock(before, after, report).unwrap();
    }

    #[test]
    fn prompt_manifest_exposes_stable_received_block_ids() {
        let text = "# Titulo\n\nParagrafo aprovado.\n\n- item";

        let blocks = segment_editorial_blocks(text);
        let manifest = format_block_manifest_for_prompt(text);

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].id, "B0001");
        assert_eq!(blocks[1].id, "B0002");
        assert_eq!(blocks[2].kind, "list");
        assert!(manifest.contains("| B0001 | heading |"));
        assert!(manifest.contains("| B0002 | paragraph |"));
        assert!(manifest.contains("locked_by_default"));
    }
}
