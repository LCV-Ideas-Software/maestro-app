use std::collections::{BTreeMap, VecDeque};

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
    let reordered_ids = reordered_received_block_ids(&before_blocks, &after_blocks);
    let reordered = !reordered_ids.is_empty();
    let Some(changed_section) = extract_changed_blocks_section(report) else {
        if changed_ids.is_empty() && after_blocks.len() <= before_blocks.len() && !reordered {
            return Ok(());
        }
        if reordered {
            return Err(format!(
                "approved-content lock violation: revised custody reordered received blocks {} but maestro_revision_report has no changed_blocks section with change_type reorder",
                reordered_ids.join(", ")
            ));
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

    let mut ids_requiring_protocol_basis = changed_ids.clone();
    for id in &reordered_ids {
        if !ids_requiring_protocol_basis.contains(id) {
            ids_requiring_protocol_basis.push(id.clone());
        }
    }

    let missing_protocol_basis = ids_requiring_protocol_basis
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

    if reordered
        && !reordered_ids.iter().all(|id| {
            declarations
                .get(id)
                .map(|declaration| declaration.allows_reorder)
                .unwrap_or(false)
        })
    {
        let missing_reorder = reordered_ids
            .iter()
            .filter(|id| {
                declarations
                    .get(*id)
                    .map(|declaration| !declaration.allows_reorder)
                    .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        return Err(format!(
            "approved-content lock violation: reordered received blocks {} must each declare change_type reorder in changed_blocks",
            missing_reorder.join(", ")
        ));
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

fn reordered_received_block_ids(
    before_blocks: &[EditorialContentBlock],
    after_blocks: &[EditorialContentBlock],
) -> Vec<String> {
    let common_counts = common_normalized_hash_counts(before_blocks, after_blocks);
    if common_counts.values().sum::<usize>() <= 1 {
        return Vec::new();
    }

    let before_sequence = common_block_id_sequence(before_blocks, before_blocks, &common_counts);
    let after_sequence = common_block_id_sequence(before_blocks, after_blocks, &common_counts);
    if before_sequence == after_sequence {
        return Vec::new();
    }

    let before_positions = before_sequence
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let after_positions = after_sequence
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect::<BTreeMap<_, _>>();

    before_sequence
        .into_iter()
        .filter(|id| before_positions.get(id) != after_positions.get(id))
        .collect()
}

fn common_normalized_hash_counts(
    before_blocks: &[EditorialContentBlock],
    after_blocks: &[EditorialContentBlock],
) -> BTreeMap<String, usize> {
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
    common_counts
}

fn common_block_id_sequence(
    before_blocks: &[EditorialContentBlock],
    ordered_blocks: &[EditorialContentBlock],
    common_counts: &BTreeMap<String, usize>,
) -> Vec<String> {
    let mut ids_by_hash = BTreeMap::<String, VecDeque<String>>::new();
    let mut remaining = common_counts.clone();
    for block in before_blocks {
        if let Some(count) = remaining.get_mut(&block.normalized_hash) {
            if *count > 0 {
                ids_by_hash
                    .entry(block.normalized_hash.clone())
                    .or_default()
                    .push_back(block.id.clone());
                *count -= 1;
            }
        }
    }

    let mut remaining = common_counts.clone();
    let mut sequence = Vec::new();
    for block in ordered_blocks {
        if let Some(count) = remaining.get_mut(&block.normalized_hash) {
            if *count > 0 {
                if let Some(ids) = ids_by_hash.get_mut(&block.normalized_hash) {
                    if let Some(id) = ids.pop_front() {
                        sequence.push(id);
                    }
                }
                *count -= 1;
            }
        }
    }
    sequence
}

fn extract_changed_blocks_section(report: &str) -> Option<&str> {
    let lower = report.to_ascii_lowercase();
    let start = find_first_report_field_key(&lower, &["changed_blocks", "changes"])?;
    let relative_end = find_first_report_field_key(
        &lower[start + 1..],
        &[
            "operator_evidence_required",
            "out_of_scope",
            "quality_preservation",
            "unchanged_approved_blocks",
            "custody",
        ],
    );
    let end = relative_end
        .map(|offset| start + 1 + offset)
        .unwrap_or(report.len());
    report.get(start..end)
}

fn find_first_report_field_key(haystack: &str, keys: &[&str]) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let mut index = 0usize;
    let mut in_quote: Option<u8> = None;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote) = in_quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == quote {
                in_quote = None;
            }
            index += 1;
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            let field_start = index;
            if let Some(end_quote) = haystack[index + 1..].find(byte as char) {
                let key_start = index + 1;
                let key_end = key_start + end_quote;
                let candidate = &haystack[key_start..key_end];
                let after = key_end + 1;
                if keys.contains(&candidate)
                    && field_key_is_delimited_before(bytes, field_start)
                    && field_key_has_assignment_after(bytes, after)
                {
                    return Some(field_start);
                }
            }
            in_quote = Some(byte);
            index += 1;
            continue;
        }
        if field_key_is_delimited_before(bytes, index) {
            for key in keys {
                if haystack[index..].starts_with(key) {
                    let after = index + key.len();
                    if field_key_has_assignment_after(bytes, after) {
                        return Some(index);
                    }
                }
            }
        }
        index += 1;
    }
    None
}

fn field_key_is_delimited_before(bytes: &[u8], index: usize) -> bool {
    if index == 0 {
        return true;
    }
    bytes[..index]
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())
        .map(|byte| matches!(byte, b'{' | b'[' | b',' | b'\n' | b'\r'))
        .unwrap_or(true)
}

fn field_key_has_assignment_after(bytes: &[u8], index: usize) -> bool {
    bytes[index..]
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .map(|byte| matches!(byte, b':' | b'='))
        .unwrap_or(false)
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
    let Some(matched) = Regex::new(r#"(?is)["']?protocol_basis["']?\s*[:=]\s*"#)
        .expect("valid protocol_basis key regex")
        .find(fragment)
    else {
        return false;
    };
    protocol_basis_value_is_nonempty(&fragment[matched.end()..])
}

fn protocol_basis_value_is_nonempty(value: &str) -> bool {
    let value = value.trim_start();
    if value.is_empty() {
        return false;
    }
    if let Some(rest) = value.strip_prefix('"') {
        return quoted_value_is_nonempty(rest, '"');
    }
    if let Some(rest) = value.strip_prefix('\'') {
        return quoted_value_is_nonempty(rest, '\'');
    }
    if let Some(rest) = value.strip_prefix('[') {
        return bracketed_value_is_nonempty(rest, '[', ']');
    }
    if let Some(rest) = value.strip_prefix('{') {
        return bracketed_value_is_nonempty(rest, '{', '}');
    }
    let token = value
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | '}' | ']'))
        .next()
        .unwrap_or("")
        .trim();
    !token.is_empty() && !token.eq_ignore_ascii_case("null") && token != "[]" && token != "{}"
}

fn quoted_value_is_nonempty(rest: &str, quote: char) -> bool {
    let mut escaped = false;
    let mut value = String::new();
    for character in rest.chars() {
        if escaped {
            value.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            return !value.trim().is_empty();
        }
        value.push(character);
    }
    false
}

fn bracketed_value_is_nonempty(rest: &str, open: char, close: char) -> bool {
    let mut depth = 1usize;
    let mut body = String::new();
    let mut in_quote: Option<char> = None;
    let mut escaped = false;
    for character in rest.chars() {
        if let Some(quote) = in_quote {
            body.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == quote {
                in_quote = None;
            }
            continue;
        }
        if character == '"' || character == '\'' {
            in_quote = Some(character);
            body.push(character);
            continue;
        }
        if character == open {
            depth += 1;
            body.push(character);
            continue;
        }
        if character == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return !body.trim().is_empty();
            }
            body.push(character);
            continue;
        }
        body.push(character);
    }
    false
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
    fn empty_structured_protocol_basis_is_rejected() {
        let before = "# Titulo\n\nParagrafo aprovado.";
        let after = "# Titulo\n\nParagrafo encurtado.";
        let array_report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "protocol_basis": []}
          ],
          "custody": "revised"
        }"#;
        let object_report = r#"{
          "changed_blocks": [
            {"block_id": "B0002", "protocol_basis": {}}
          ],
          "custody": "revised"
        }"#;

        let array_error = validate_revision_content_lock(before, after, array_report).unwrap_err();
        let object_error =
            validate_revision_content_lock(before, after, object_report).unwrap_err();

        assert!(array_error.contains("protocol_basis"), "{array_error}");
        assert!(object_error.contains("protocol_basis"), "{object_error}");
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
    fn silent_reorder_without_changed_blocks_section_is_rejected() {
        let before = "# Titulo\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.";
        let after = "# Titulo\n\nSegundo bloco aprovado.\n\nPrimeiro bloco aprovado.";
        let report = r#"{
          "reviewer": "gemini",
          "status": "READY",
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("reordered received blocks"), "{error}");
    }

    #[test]
    fn reorder_declaration_must_name_moved_received_blocks() {
        let before = "# Titulo\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.";
        let after = "# Titulo\n\nSegundo bloco aprovado.\n\nPrimeiro bloco aprovado.";
        let report = r#"{
          "changed_blocks": [
            {
              "block_id": "B0001",
              "change_type": "reorder",
              "protocol_basis": "structure"
            }
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("B0002"), "{error}");
        assert!(error.contains("B0003"), "{error}");
        assert!(error.contains("reorder"), "{error}");
    }

    #[test]
    fn declared_reorder_for_each_moved_received_block_is_allowed() {
        let before = "# Titulo\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.";
        let after = "# Titulo\n\nSegundo bloco aprovado.\n\nPrimeiro bloco aprovado.";
        let report = r#"{
          "changed_blocks": [
            {
              "block_id": "B0002",
              "change_type": "reorder",
              "protocol_basis": "structure"
            },
            {
              "block_id": "B0003",
              "change_type": "reorder",
              "protocol_basis": "structure"
            }
          ],
          "custody": "revised"
        }"#;

        validate_revision_content_lock(before, after, report).unwrap();
    }

    #[test]
    fn duplicate_identical_blocks_do_not_hide_distinct_reorder_requirements() {
        let before = "# Titulo\n\nParagrafo repetido aprovado.\n\nBloco medio aprovado.\n\nParagrafo repetido aprovado.\n\nConclusao aprovada.";
        let after = "# Titulo\n\nBloco medio aprovado.\n\nParagrafo repetido aprovado.\n\nParagrafo repetido aprovado.\n\nConclusao aprovada.";
        let report = r#"{
          "changed_blocks": [
            {
              "block_id": "B0003",
              "change_type": "reorder",
              "protocol_basis": "structure"
            }
          ],
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("B0002"), "{error}");
        assert!(error.contains("reorder"), "{error}");
    }

    #[test]
    fn changed_section_terminators_match_fields_not_value_text() {
        let before = "# Titulo\n\nParagrafo aprovado.";
        let after = "# Titulo\n\nParagrafo corrigido.";
        let report = r#"{
          "changed_blocks": [
            {
              "block_id": "B0002",
              "reason": "clarifies custody transfer without changing scope",
              "protocol_basis": "editorial precision"
            }
          ],
          "custody": "revised"
        }"#;

        validate_revision_content_lock(before, after, report).unwrap();
    }

    #[test]
    fn changed_section_terminators_ignore_escaped_quotes_inside_value_text() {
        let before = "# Titulo\n\nParagrafo aprovado.";
        let after = "# Titulo\n\nParagrafo corrigido.";
        let report = r#"{
          "changed_blocks": [
            {
              "block_id": "B0002",
              "reason": "clarifies \"custody\" transfer without changing scope",
              "protocol_basis": "editorial precision with escaped \"custody\" text"
            }
          ],
          "custody": "revised"
        }"#;

        validate_revision_content_lock(before, after, report).unwrap();
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
    fn moved_and_edited_block_still_requires_changed_blocks_protocol_basis() {
        let before = "# Titulo\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.\n\nTerceiro bloco aprovado.";
        let after = "# Titulo\n\nTerceiro bloco editado.\n\nPrimeiro bloco aprovado.\n\nSegundo bloco aprovado.";
        let report = r#"{
          "reviewer": "grok",
          "status": "READY",
          "custody": "revised"
        }"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();

        assert!(error.contains("B0004"), "{error}");
        assert!(error.contains("changed_blocks"), "{error}");
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
