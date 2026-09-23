use std::collections::{BTreeMap, VecDeque};

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;
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
    allowed_block_count_growth: usize,
    allows_reorder: bool,
}

#[derive(Deserialize)]
struct RevisionReport {
    #[serde(default)]
    changed_blocks: Vec<ChangedBlockEntry>,
}

#[derive(Deserialize)]
struct ChangedBlockEntry {
    block_id: String,
    #[serde(default)]
    protocol_basis: Option<Value>,
    #[serde(default)]
    change_type: Option<String>,
    #[serde(default)]
    new_block_count: Option<u64>,
}

pub(crate) fn segment_editorial_blocks(text: &str) -> Vec<EditorialContentBlock> {
    let normalized_newlines = text.replace("\r\n", "\n").replace('\r', "\n");
    let blank_line = Regex::new(r"\n[\t ]*\n").expect("valid blank-line regex");
    blank_line
        .split(&normalized_newlines)
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
    // Serde can deserialize a struct from a positional JSON array. The report
    // contract requires an object, so check the root before typed decoding.
    let root: Value = serde_json::from_str(report).map_err(|error| {
        format!(
            "approved-content lock violation: maestro_revision_report must be one strict JSON object: {error}"
        )
    })?;
    if !root.is_object() {
        return Err(
            "approved-content lock violation: maestro_revision_report must be one strict JSON object"
                .to_string(),
        );
    }
    let parsed: RevisionReport = serde_json::from_str(report).map_err(|error| {
        format!(
            "approved-content lock violation: maestro_revision_report must be one strict JSON object: {error}"
        )
    })?;
    let before_blocks = segment_editorial_blocks(before);
    let after_blocks = segment_editorial_blocks(after);
    let changed_ids = changed_received_block_ids(&before_blocks, &after_blocks);
    let reordered_ids = reordered_received_block_ids(&before_blocks, &after_blocks);
    let reordered = !reordered_ids.is_empty();
    if parsed.changed_blocks.is_empty() {
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
    }
    let declarations = parse_changed_block_declarations(parsed.changed_blocks, &before_blocks)?;

    let mut before_hash_counts = BTreeMap::<&str, usize>::new();
    let mut after_hash_counts = BTreeMap::<&str, usize>::new();
    for block in &before_blocks {
        *before_hash_counts
            .entry(block.normalized_hash.as_str())
            .or_insert(0) += 1;
    }
    for block in &after_blocks {
        *after_hash_counts
            .entry(block.normalized_hash.as_str())
            .or_insert(0) += 1;
    }
    let before_hash_by_id = before_blocks
        .iter()
        .map(|block| (block.id.as_str(), block.normalized_hash.as_str()))
        .collect::<BTreeMap<_, _>>();
    let ambiguous_duplicate_ids = changed_ids
        .iter()
        .filter(|id| {
            let Some(hash) = before_hash_by_id.get(id.as_str()) else {
                return false;
            };
            let original_count = before_hash_counts.get(*hash).copied().unwrap_or(0);
            original_count > 1
                && after_hash_counts.get(*hash).copied().unwrap_or(0) < original_count
        })
        .cloned()
        .collect::<Vec<_>>();
    if !ambiguous_duplicate_ids.is_empty() {
        return Err(format!(
            "approved-content lock violation: changed duplicate received blocks {} have ambiguous attribution",
            ambiguous_duplicate_ids.join(", ")
        ));
    }

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

    let added_blocks = after_blocks.len().saturating_sub(before_blocks.len());
    let allowed_growth = declarations.values().fold(0usize, |total, declaration| {
        total.saturating_add(if declaration.has_protocol_basis {
            declaration.allowed_block_count_growth
        } else {
            0
        })
    });
    if added_blocks > allowed_growth {
        return Err(
            "approved-content lock violation: revised custody added new blocks beyond per-block change_type split/addition permissions in changed_blocks"
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
    // Bind equal blocks at both edges before matching the interior. This keeps
    // repeated text at the other edge from consuming an edited block's ID.
    let mut start = 0;
    while start < before_blocks.len()
        && start < after_blocks.len()
        && before_blocks[start].normalized_hash == after_blocks[start].normalized_hash
    {
        start += 1;
    }
    let mut before_end = before_blocks.len();
    let mut after_end = after_blocks.len();
    while before_end > start
        && after_end > start
        && before_blocks[before_end - 1].normalized_hash
            == after_blocks[after_end - 1].normalized_hash
    {
        before_end -= 1;
        after_end -= 1;
    }
    let mut after_hash_counts = BTreeMap::<&str, usize>::new();
    for after in &after_blocks[start..after_end] {
        *after_hash_counts
            .entry(after.normalized_hash.as_str())
            .or_insert(0) += 1;
    }

    before_blocks[start..before_end]
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

fn parse_changed_block_declarations(
    entries: Vec<ChangedBlockEntry>,
    before_blocks: &[EditorialContentBlock],
) -> Result<BTreeMap<String, ChangedBlockDeclaration>, String> {
    let mut declarations = BTreeMap::new();
    for entry in entries {
        let digits = entry.block_id.strip_prefix('B').unwrap_or("");
        if digits.len() < 4 || !digits.bytes().all(|digit| digit.is_ascii_digit()) {
            return Err(format!(
                "approved-content lock violation: invalid changed_blocks block_id {}",
                entry.block_id
            ));
        }
        if !before_blocks.iter().any(|block| block.id == entry.block_id) {
            return Err(format!(
                "approved-content lock violation: changed_blocks block_id {} is absent from the received manifest",
                entry.block_id
            ));
        }
        if declarations.contains_key(&entry.block_id) {
            return Err(format!(
                "approved-content lock violation: duplicate changed_blocks declaration for {}",
                entry.block_id
            ));
        }

        let has_protocol_basis = match entry.protocol_basis {
            Some(Value::String(value)) => !value.trim().is_empty(),
            Some(Value::Array(values)) => !values.is_empty(),
            Some(Value::Object(values)) => !values.is_empty(),
            _ => false,
        };
        let allowed_block_count_growth = if matches!(
            entry.change_type.as_deref(),
            Some("split" | "addition")
        ) {
            match entry.new_block_count {
                Some(count) => usize::try_from(count).unwrap_or(0),
                None => 1,
            }
        } else {
            0
        };
        declarations.insert(
            entry.block_id,
            ChangedBlockDeclaration {
                has_protocol_basis,
                allowed_block_count_growth,
                allows_reorder: entry.change_type.as_deref() == Some("reorder"),
            },
        );
    }
    Ok(declarations)
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
    fn addition_declared_on_unrelated_block_cannot_authorize_insertion() {
        let before = "Primeiro.\n\nSegundo.\n\nTerceiro.";
        let after = "Primeiro.\n\nNovo.\n\nSegundo.\n\nTerceiro.";
        let report = r#"{"changed_blocks":[
            {"block_id":"B0003","change_type":"addition","protocol_basis":"required context"}
        ],"custody":"revised"}"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();
        assert!(error.contains("B0001") || error.contains("B0002"), "{error}");
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
    fn one_block_can_declare_addition_and_reorder_together() {
        let before = "Primeiro.\n\nSegundo.";
        let after = "Novo.\n\nSegundo.\n\nPrimeiro.";
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","change_type":"reorder","protocol_basis":"structure"},
            {"block_id":"B0002","change_type":["addition","reorder"],"protocol_basis":"structure and context"}
        ],"custody":"revised"}"#;

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
    fn prose_words_cannot_authorize_reorder() {
        let before = "Primeiro.\n\nSegundo.";
        let after = "Segundo.\n\nPrimeiro.";
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","change_type":"edit","reason":"removed punctuation","protocol_basis":"structure"},
            {"block_id":"B0002","change_type":"edit","reason":"nothing moved","protocol_basis":"structure"}
        ]}"#;

        let error = validate_revision_content_lock(before, after, report).unwrap_err();
        assert!(error.contains("reorder"), "{error}");
    }

    #[test]
    fn protocol_basis_in_reason_does_not_authorize_an_edit() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","reason":"protocol_basis: not supplied"}
        ]}"#;

        let error = validate_revision_content_lock("Original.", "Revisado.", report).unwrap_err();
        assert!(error.contains("protocol_basis"), "{error}");
    }

    #[test]
    fn one_declared_addition_cannot_authorize_two_new_blocks() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","change_type":"addition","protocol_basis":"required context"}
        ]}"#;

        let error = validate_revision_content_lock(
            "Original.",
            "Original.\n\nNovo um.\n\nNovo dois.",
            report,
        )
        .unwrap_err();
        assert!(error.contains("added new blocks"), "{error}");
    }

    #[test]
    fn negated_addition_in_reason_cannot_grant_growth() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","change_type":"edit","reason":"no addition was made","protocol_basis":"style"}
        ]}"#;

        let error =
            validate_revision_content_lock("Original.", "Original.\n\nNovo.", report)
                .unwrap_err();
        assert!(error.contains("added new blocks"), "{error}");
    }

    #[test]
    fn explicit_positive_count_can_authorize_two_new_blocks() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","change_type":"addition","new_block_count":2,"protocol_basis":"required context"}
        ]}"#;

        validate_revision_content_lock(
            "Original.",
            "Original.\n\nNovo um.\n\nNovo dois.",
            report,
        )
        .unwrap();
    }

    #[test]
    fn duplicate_received_text_with_an_edit_is_ambiguous() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B0002","protocol_basis":"correction"}
        ]}"#;

        let error =
            validate_revision_content_lock("Alpha\n\nAlpha", "Novo\n\nAlpha", report)
                .unwrap_err();
        assert!(error.contains("ambiguous"), "{error}");
    }

    #[test]
    fn report_requires_one_strict_json_object_even_without_changes() {
        for report in [
            "Resumo das mudanças: {\"changed_blocks\":[]}",
            "changed_blocks:\n  - block_id: B0001",
            "{\"changed_blocks\":[]} trailing",
            "[[]]",
        ] {
            let error = validate_revision_content_lock("Original.", "Original.", report)
                .unwrap_err();
            assert!(error.contains("strict JSON object"), "{error}");
        }
    }

    #[test]
    fn received_block_b10000_can_be_declared() {
        let before = (1..=10_000)
            .map(|index| format!("Bloco {index}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let after = format!(
            "{}\n\nBloco 10000 revisado.",
            before.rsplit_once("\n\n").unwrap().0
        );
        let report = r#"{"changed_blocks":[
            {"block_id":"B10000","protocol_basis":"editorial correction"}
        ]}"#;

        validate_revision_content_lock(&before, &after, report).unwrap();
    }

    #[test]
    fn braces_and_escaped_quotes_inside_reason_do_not_break_json_fields() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B0001","reason":"literal { and \"quoted } text\"","protocol_basis":"correction"}
        ]}"#;

        validate_revision_content_lock("Original.", "Revisado.", report).unwrap();
    }

    #[test]
    fn duplicate_block_declarations_and_duplicate_json_fields_fail_closed() {
        let duplicate_entries = r#"{"changed_blocks":[
            {"block_id":"B0001","protocol_basis":"correction"},
            {"block_id":"B0001","change_type":"addition","protocol_basis":"correction"}
        ]}"#;
        let duplicate_field = r#"{"changed_blocks":[
            {"block_id":"B0001","block_id":"B0002","protocol_basis":"correction"}
        ]}"#;

        let error =
            validate_revision_content_lock("Original.", "Revisado.", duplicate_entries)
                .unwrap_err();
        assert!(error.contains("duplicate changed_blocks"), "{error}");
        let error = validate_revision_content_lock("Original.", "Revisado.", duplicate_field)
            .unwrap_err();
        assert!(error.contains("strict JSON object"), "{error}");
    }

    #[test]
    fn nonexistent_received_block_cannot_grant_growth() {
        let report = r#"{"changed_blocks":[
            {"block_id":"B9999","change_type":"addition","protocol_basis":"required context"}
        ]}"#;

        let error =
            validate_revision_content_lock("Original.", "Original.\n\nNovo.", report)
                .unwrap_err();
        assert!(error.contains("received manifest"), "{error}");
    }

    #[test]
    fn whitespace_only_separator_line_creates_distinct_blocks() {
        let before = "Primeiro.\n   \nSegundo.";
        let after = "Primeiro.\n   \nSegundo revisado.";
        let report = r#"{"changed_blocks":[
            {"block_id":"B0002","protocol_basis":"correction"}
        ]}"#;

        assert_eq!(segment_editorial_blocks(before).len(), 2);
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
