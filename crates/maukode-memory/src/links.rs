//! Parsing of `[[Title]]` / `[[rel::Title]]` wiki-links out of memory content.
//!
//! Links inside fenced or inline code spans are ignored so documentation that discusses the
//! syntax itself never fabricates edges.

use crate::models::LinkRelation;

/// A parsed outgoing link, before resolution against existing titles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    pub rel: LinkRelation,
    pub raw_target: String,
    pub normalized_target: String,
}

/// Normalize a link target for matching: lowercase, collapse whitespace, trim.
pub fn normalize_target(raw: &str) -> String {
    raw.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Extract the links from `content`, honoring code-span exclusions and collapsing duplicate
/// `(rel, target)` pairs to one.
pub fn parse_links(content: &str) -> Vec<ParsedLink> {
    let masked = mask_code_spans(content);
    let mut links: Vec<ParsedLink> = Vec::new();
    let bytes = masked.as_bytes();
    let mut i = 0;

    while i + 1 < bytes.len() {
        if bytes[i] == b'[' && bytes[i + 1] == b'[' {
            if let Some(close) = find_close(&masked, i + 2) {
                let inner = &masked[i + 2..close];
                if let Some(link) = parse_inner(inner) {
                    let duplicate = links.iter().any(|existing| {
                        existing.rel == link.rel
                            && existing.normalized_target == link.normalized_target
                    });
                    if !duplicate {
                        links.push(link);
                    }
                }
                i = close + 2;
                continue;
            }
        }
        i += 1;
    }

    links
}

fn find_close(text: &str, from: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn parse_inner(inner: &str) -> Option<ParsedLink> {
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return None;
    }

    // A typed link splits once on `::`. A recognized prefix becomes the relation; anything else
    // (including a stray `::` in a title) degrades to the default relation on the literal text.
    let (rel, target) = match trimmed.split_once("::") {
        Some((prefix, rest)) => {
            let prefix = prefix.trim();
            match LinkRelation::parse(prefix) {
                Some(relation) => (relation, rest.trim()),
                None => (LinkRelation::Relates, trimmed),
            }
        }
        None => (LinkRelation::Relates, trimmed),
    };

    if target.is_empty() {
        return None;
    }

    Some(ParsedLink {
        rel,
        raw_target: target.to_string(),
        normalized_target: normalize_target(target),
    })
}

/// Replace the contents of fenced (``` ... ```) and inline (`...`) code spans with spaces so the
/// link scanner cannot see brackets inside them. Preserves byte length so offsets stay valid.
fn mask_code_spans(content: &str) -> String {
    let mut out: Vec<u8> = content.as_bytes().to_vec();
    let bytes = out.clone();
    let len = bytes.len();

    // Fenced code blocks: from a ``` line to the next ``` line.
    let mut i = 0;
    while i + 2 < len {
        if bytes[i] == b'`' && bytes[i + 1] == b'`' && bytes[i + 2] == b'`' {
            let start = i;
            let mut j = i + 3;
            let mut closed = false;
            while j + 2 < len {
                if bytes[j] == b'`' && bytes[j + 1] == b'`' && bytes[j + 2] == b'`' {
                    j += 3;
                    closed = true;
                    break;
                }
                j += 1;
            }
            let end = if closed { j } else { len };
            for byte in out.iter_mut().take(end).skip(start) {
                if *byte != b'\n' {
                    *byte = b' ';
                }
            }
            i = end;
            continue;
        }
        i += 1;
    }

    // Inline code spans: single backtick pairs (not part of a fence).
    let mut i = 0;
    while i < len {
        if bytes[i] == b'`' {
            let mut j = i + 1;
            while j < len && bytes[j] != b'`' && bytes[j] != b'\n' {
                j += 1;
            }
            if j < len && bytes[j] == b'`' {
                for byte in out.iter_mut().take(j + 1).skip(i) {
                    if *byte != b'\n' {
                        *byte = b' ';
                    }
                }
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }

    String::from_utf8(out).unwrap_or_else(|_| content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_link_defaults_to_relates() {
        let links = parse_links("see [[Acme Onboarding]] for details");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].rel, LinkRelation::Relates);
        assert_eq!(links[0].raw_target, "Acme Onboarding");
        assert_eq!(links[0].normalized_target, "acme onboarding");
    }

    #[test]
    fn typed_links_parse_relation() {
        let links =
            parse_links("[[depends-on::Q3 Launch]] [[supersedes::Old Plan]] [[contradicts::Note]]");
        let rels: Vec<_> = links.iter().map(|l| l.rel).collect();
        assert_eq!(
            rels,
            vec![
                LinkRelation::DependsOn,
                LinkRelation::Supersedes,
                LinkRelation::Contradicts
            ]
        );
    }

    #[test]
    fn unknown_prefix_degrades_to_relates_on_literal_text() {
        let links = parse_links("[[needs::Foo]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].rel, LinkRelation::Relates);
        assert_eq!(links[0].raw_target, "needs::Foo");
    }

    #[test]
    fn links_in_code_spans_are_ignored() {
        let content = "real [[Real]] and `[[NotALink]]`\n```\n[[AlsoNot]]\n```\n";
        let links = parse_links(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].raw_target, "Real");
    }

    #[test]
    fn duplicate_pairs_collapse() {
        let links = parse_links("[[Foo]] [[Foo]] [[relates::foo]]");
        assert_eq!(links.len(), 1);
    }
}
