//! Independent citation validation for the HTML that the native draft and D1
//! commands actually persist. The renderer's Markdown projection is useful for
//! feedback, but it is not a publication authority.

use scraper::{ElementRef, Html};
use serde::{Deserialize, Serialize};

use crate::abnt_citation::{
    audit_abnt_citations_inner, AbntAuditRequest, CitationManifest, MaestroPeerStatus,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct MainSiteCitationContext {
    pub(crate) protocol_hash: Option<String>,
    pub(crate) manifest: Option<CitationManifest>,
    pub(crate) previous_manifest: Option<CitationManifest>,
}

fn escape_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_whitespace() {
            if !escaped.ends_with(' ') {
                escaped.push(' ');
            }
            continue;
        }
        if matches!(
            character,
            '\\' | '`'
                | '~'
                | '*'
                | '_'
                | '['
                | ']'
                | '{'
                | '}'
                | '#'
                | '+'
                | '!'
                | '|'
                | '<'
                | '>'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn children(element: ElementRef<'_>, depth: usize) -> Result<String, String> {
    if depth > 256 {
        return Err("MAINSITE_CITATION_HTML_DEPTH_EXCEEDED".to_string());
    }
    let mut rendered = String::new();
    for child in element.children() {
        if let Some(text) = child.value().as_text() {
            rendered.push_str(&escape_text(text));
        } else if let Some(child) = ElementRef::wrap(child) {
            rendered.push_str(&render_element(child, depth + 1)?);
        }
    }
    Ok(rendered)
}

fn render_element(element: ElementRef<'_>, depth: usize) -> Result<String, String> {
    let tag = element.value().name();
    if matches!(tag, "script" | "style" | "noscript" | "template") {
        return Err("MAINSITE_CITATION_UNSUPPORTED_HTML".to_string());
    }
    if matches!(tag, "code" | "pre") {
        // Code examples are outside the ABNT body-text audit. Omitting them
        // avoids creating a Markdown fence from otherwise safe HTML nesting.
        return Ok(" ".to_string());
    }
    if tag == "img" {
        return Ok(String::new());
    }
    let content = children(element, depth)?;
    let result = match tag {
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let level = tag.as_bytes()[1] - b'0';
            format!("{} {}\n\n", "#".repeat(usize::from(level)), content.trim())
        }
        "p" => format!("{}\n\n", content.trim()),
        "strong" | "b" => format!("**{content}**"),
        "em" | "i" => format!("*{content}*"),
        "s" | "del" => content,
        "blockquote" => format!(
            "{}\n\n",
            content
                .trim()
                .lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "li" => format!("- {}\n", content.trim()),
        "ul" | "ol" => format!("{content}\n"),
        "a" => match element.value().attr("href") {
            Some(url) => format!("[{content}](<{}>)", url.replace('>', "%3E")),
            None => content,
        },
        "br" => "  \n".to_string(),
        "hr" => "\n---\n\n".to_string(),
        "tr" => format!("{}\n", content.trim_end()),
        "td" | "th" => format!("{} ", content.trim()),
        "table" | "figure" => format!("{content}\n\n"),
        _ => content,
    };
    Ok(result)
}

pub(crate) fn html_to_citation_markdown(html: &str) -> Result<String, String> {
    let document = Html::parse_fragment(html);
    let text = children(document.root_element(), 0)?;
    Ok(text
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string())
}

pub(crate) fn require_mainsite_citations_ready(
    html: &str,
    context: Option<&MainSiteCitationContext>,
) -> Result<(), String> {
    let context = context.ok_or_else(|| "MAINSITE_CITATION_CONTEXT_REQUIRED".to_string())?;
    let result = audit_abnt_citations_inner(AbntAuditRequest {
        text: html_to_citation_markdown(html)?,
        protocol_hash: context.protocol_hash.clone(),
        manifest: context.manifest.clone(),
        previous_manifest: context.previous_manifest.clone(),
    })?;
    if result.maestro_peer_status != MaestroPeerStatus::Ready || !result.blockers.is_empty() {
        return Err(format!(
            "MAINSITE_CITATION_GATE_BLOCKED: {} blocker(s)",
            result.blockers.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> MainSiteCitationContext {
        MainSiteCitationContext {
            protocol_hash: None,
            manifest: None,
            previous_manifest: None,
        }
    }

    #[test]
    fn native_gate_rejects_missing_context_and_unlisted_citation() {
        let html = "<p>Silva (2026) descreve o resultado.</p><h2>Referências</h2><p>SILVA. Obra. 2026.</p>";
        assert_eq!(
            require_mainsite_citations_ready(html, None).unwrap_err(),
            "MAINSITE_CITATION_CONTEXT_REQUIRED"
        );
        assert!(require_mainsite_citations_ready(html, Some(&context())).is_err());
        assert!(require_mainsite_citations_ready(
            "<p>Texto autoral sem citação.</p>",
            Some(&context())
        )
        .is_ok());
    }

    #[test]
    fn html_projection_preserves_headings_and_omits_code_examples() {
        let html = "<p><code>example`` (Silva, 2026)</code></p><h2>Referências</h2><table><tr><td>Uma</td><td>Duas</td></tr></table>";
        let markdown = html_to_citation_markdown(html).unwrap();
        assert!(!markdown.contains("example`` (Silva, 2026)"));
        assert!(markdown.contains("## Referências"));
        assert!(markdown.contains("Uma Duas"));
        let code_only = "<p><code>example`` (Silva, 2026)</code></p><p>Texto autoral.</p>";
        assert!(require_mainsite_citations_ready(code_only, Some(&context())).is_ok());
        let following_citation = "<p><code>\n``x</code></p><p>(Silva, 2026)</p>";
        assert!(html_to_citation_markdown(following_citation)
            .unwrap()
            .contains("(Silva, 2026)"));
        assert!(require_mainsite_citations_ready(following_citation, Some(&context())).is_err());
    }

    #[test]
    fn html_projection_cannot_hide_later_citations_with_markdown_fences() {
        for html in [
            "<p>~~~</p><p>(Silva, 2026)</p>",
            "<p><s>~</s></p><p>(Silva, 2026)</p>",
            "<p><s><s>x</s></s></p><p>(Silva, 2026)</p>",
            "<p><del><s>x</s></del></p><p>(Silva, 2026)</p>",
            "<p><s><s>x</s>y</s></p><p>(Silva, 2026)</p>",
            "<s><s>x</s></s><p>(Silva, 2026)</p>",
            "<p><code></code>(Silva, 2026)<code></code></p>",
            "<ul><li><pre>~~~</pre></li></ul><p>(Silva, 2026)</p>",
            "<h2><pre>~~~</pre></h2><p>(Silva, 2026)</p>",
            "<p>Texto.</p>    (Silva, 2026)",
            "<p>A</p> <img src=\"https://e/x.png\"> <img src=\"https://e/x.png\"> <img src=\"https://e/x.png\"> <img src=\"https://e/x.png\"> (Silva, 2026)",
            "<p>A</p><sup> </sup><sup> </sup><sup> </sup><sup> </sup>(Silva, 2026)",
        ] {
            let markdown = html_to_citation_markdown(html).unwrap();
            assert!(markdown.contains("(Silva, 2026)"), "{html}: {markdown}");
            assert!(
                require_mainsite_citations_ready(html, Some(&context())).is_err(),
                "{html}: {markdown}"
            );
        }
    }
}
