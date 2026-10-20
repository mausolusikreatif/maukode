//! `@file` completion in the prompt: find the mention under the cursor, rank workspace files
//! against it, and put the chosen path in its place.

/// Most completions shown at once.
pub const MAX_SUGGESTIONS: usize = 8;

/// An `@` mention being typed: where its `@` is (a character index) and the text after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub start: usize,
    pub query: String,
}

/// The mention that ends at `cursor` (a character index), if any: an `@` at the start of the
/// input or after whitespace, followed by non-whitespace up to the cursor.
pub fn mention_at(input: &str, cursor: usize) -> Option<Mention> {
    let before: Vec<char> = input.chars().take(cursor).collect();
    let word_start = before
        .iter()
        .rposition(|c| c.is_whitespace())
        .map_or(0, |i| i + 1);
    if before.get(word_start) != Some(&'@') {
        return None;
    }
    Some(Mention {
        start: word_start,
        query: before[word_start + 1..].iter().collect(),
    })
}

/// Files matching `query` as a case-insensitive subsequence, best first: matches in the file name
/// and runs of consecutive characters rank higher, shorter paths break ties.
pub fn rank(query: &str, files: &[String], limit: usize) -> Vec<String> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    let mut scored: Vec<(i64, &String)> = files
        .iter()
        .filter_map(|path| score(&query, path).map(|s| (s, path)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.len().cmp(&b.1.len())));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, path)| path.clone())
        .collect()
}

fn score(query: &[char], path: &str) -> Option<i64> {
    let lower: Vec<char> = path.to_lowercase().chars().collect();
    let name_start = lower.iter().rposition(|&c| c == '/').map_or(0, |i| i + 1);
    let mut score = 0i64;
    let mut next = 0;
    let mut previous: Option<usize> = None;
    for &wanted in query {
        let found = (next..lower.len()).find(|&i| lower[i] == wanted)?;
        score += 1;
        if previous == Some(found.wrapping_sub(1)) {
            score += 5;
        }
        if found >= name_start {
            score += 3;
        }
        if found == name_start || found == 0 {
            score += 8;
        }
        previous = Some(found);
        next = found + 1;
    }
    Some(score * 100 - lower.len() as i64)
}

/// `input` with the mention replaced by `@path ` and the cursor (a character index) after it.
pub fn complete(input: &str, mention: &Mention, cursor: usize, path: &str) -> (String, usize) {
    let chars: Vec<char> = input.chars().collect();
    let mut out: String = chars[..mention.start].iter().collect();
    out.push('@');
    out.push_str(path);
    out.push(' ');
    let new_cursor = out.chars().count();
    let rest: String = chars[cursor.min(chars.len())..].iter().collect();
    out.push_str(rest.trim_start());
    (out, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_mention_under_the_cursor() {
        assert_eq!(
            mention_at("fix @src/ma", 11),
            Some(Mention {
                start: 4,
                query: "src/ma".into()
            })
        );
        assert_eq!(
            mention_at("@", 1),
            Some(Mention {
                start: 0,
                query: String::new()
            })
        );
        assert_eq!(mention_at("mail me@host", 12), None);
        assert_eq!(mention_at("fix @src now", 12), None);
    }

    #[test]
    fn ranks_file_name_matches_first() {
        let files: Vec<String> = [
            "docs/main-notes.md",
            "src/main.rs",
            "crates/maukode-tui/src/app.rs",
            "src/lib.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(rank("main", &files, 2), ["src/main.rs", "docs/main-notes.md"]);
        assert_eq!(rank("tuiapp", &files, 5), ["crates/maukode-tui/src/app.rs"]);
        assert!(rank("zzz", &files, 5).is_empty());
        // An empty query lists the shortest paths.
        assert_eq!(rank("", &files, 1), ["src/lib.rs"]);
    }

    #[test]
    fn completion_replaces_the_mention() {
        let mention = mention_at("look at @sr please", 11).unwrap();
        let (text, cursor) = complete("look at @sr please", &mention, 11, "src/main.rs");
        assert_eq!(text, "look at @src/main.rs please");
        assert_eq!(cursor, "look at @src/main.rs ".chars().count());
    }
}
