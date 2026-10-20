//! `apply_patch`: add, update, delete and move several files in one call.
//!
//! The patch format is the one OpenAI's coding models are trained on:
//!
//! ```text
//! *** Begin Patch
//! *** Add File: src/new.rs
//! +fn main() {}
//! *** Update File: src/lib.rs
//! *** Move to: src/core.rs
//! @@ fn helper
//!  unchanged line
//! -old line
//! +new line
//! *** Delete File: src/old.rs
//! *** End Patch
//! ```
//!
//! Every hunk is checked against the files before anything is written, so a patch applies
//! completely or not at all.

use crate::security::PathJail;
use crate::tool::{Tool, ToolError};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOp {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        hunks: Vec<Hunk>,
    },
}

/// Lines to find and their replacement, optionally after a line matching `anchor`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hunk {
    pub anchor: Option<String>,
    pub old: Vec<String>,
    pub new: Vec<String>,
}

const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File: ";
const DELETE: &str = "*** Delete File: ";
const UPDATE: &str = "*** Update File: ";
const MOVE: &str = "*** Move to: ";
const END_OF_FILE: &str = "*** End of File";

/// Parse a patch. Errors name the offending line.
pub fn parse_patch(text: &str) -> Result<Vec<PatchOp>, String> {
    let lines: Vec<&str> = text.trim().lines().collect();
    if lines.first().map(|l| l.trim()) != Some(BEGIN) {
        return Err(format!("The patch must start with `{BEGIN}`"));
    }
    if lines.last().map(|l| l.trim()) != Some(END) {
        return Err(format!("The patch must end with `{END}`"));
    }
    let body = &lines[1..lines.len() - 1];
    let mut ops = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        let at = |n: usize| format!("line {}", n + 2);
        if let Some(path) = line.strip_prefix(ADD) {
            let path = clean_path(path, at(i))?;
            let mut content = String::new();
            i += 1;
            while i < body.len() && !body[i].starts_with("*** ") {
                // An empty line stands for an empty `+` line.
                let added = match body[i].strip_prefix('+') {
                    Some(added) => added,
                    None if body[i].is_empty() => "",
                    None => {
                        return Err(format!(
                            "{}: lines of an added file must start with `+`",
                            at(i)
                        ))
                    }
                };
                content.push_str(added);
                content.push('\n');
                i += 1;
            }
            ops.push(PatchOp::Add { path, content });
        } else if let Some(path) = line.strip_prefix(DELETE) {
            ops.push(PatchOp::Delete {
                path: clean_path(path, at(i))?,
            });
            i += 1;
        } else if let Some(path) = line.strip_prefix(UPDATE) {
            let path = clean_path(path, at(i))?;
            i += 1;
            let mut move_to = None;
            if let Some(target) = body.get(i).and_then(|l| l.strip_prefix(MOVE)) {
                move_to = Some(clean_path(target, at(i))?);
                i += 1;
            }
            let mut hunks: Vec<Hunk> = Vec::new();
            while i < body.len() && !is_file_header(body[i]) {
                let line = body[i];
                if line.trim() == END_OF_FILE {
                    i += 1;
                    continue;
                }
                if let Some(anchor) = line.strip_prefix("@@") {
                    let anchor = anchor.trim();
                    hunks.push(Hunk {
                        anchor: (!anchor.is_empty()).then(|| anchor.to_string()),
                        ..Hunk::default()
                    });
                    i += 1;
                    continue;
                }
                if hunks.is_empty() {
                    hunks.push(Hunk::default());
                }
                let Some(hunk) = hunks.last_mut() else {
                    break;
                };
                match line.chars().next() {
                    Some('+') => hunk.new.push(line[1..].to_string()),
                    Some('-') => hunk.old.push(line[1..].to_string()),
                    Some(' ') => {
                        hunk.old.push(line[1..].to_string());
                        hunk.new.push(line[1..].to_string());
                    }
                    // Models often drop the space of an empty context line.
                    None => {
                        hunk.old.push(String::new());
                        hunk.new.push(String::new());
                    }
                    Some(_) => {
                        return Err(format!(
                            "{}: a hunk line must start with ` `, `-`, `+` or `@@`",
                            at(i)
                        ))
                    }
                }
                i += 1;
            }
            hunks.retain(|h| !(h.old.is_empty() && h.new.is_empty()));
            if hunks.is_empty() && move_to.is_none() {
                return Err(format!("{}: `{path}` has no changes", at(i)));
            }
            ops.push(PatchOp::Update {
                path,
                move_to,
                hunks,
            });
        } else if line.trim().is_empty() {
            i += 1;
        } else {
            return Err(format!(
                "{}: expected `{ADD}`, `{UPDATE}` or `{DELETE}`",
                at(i)
            ));
        }
    }
    if ops.is_empty() {
        return Err("The patch changes no files".into());
    }
    Ok(ops)
}

fn is_file_header(line: &str) -> bool {
    line.starts_with(ADD) || line.starts_with(DELETE) || line.starts_with(UPDATE)
}

fn clean_path(path: &str, at: String) -> Result<String, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(format!("{at}: missing file path"));
    }
    Ok(path.to_string())
}

/// Every path a patch touches, including move targets (for syncing them elsewhere).
pub fn touched_paths(ops: &[PatchOp]) -> Vec<String> {
    let mut paths = Vec::new();
    for op in ops {
        match op {
            PatchOp::Add { path, .. } | PatchOp::Delete { path } => paths.push(path.clone()),
            PatchOp::Update { path, move_to, .. } => {
                paths.push(path.clone());
                paths.extend(move_to.clone());
            }
        }
    }
    paths
}

/// Apply `hunks` to `content`. Hunks are matched in order, each after the previous one; a line
/// matches exactly, or else ignoring trailing whitespace, or else ignoring surrounding whitespace.
pub fn apply_hunks(content: &str, hunks: &[Hunk]) -> Result<String, String> {
    let had_newline = content.ends_with('\n');
    let eol = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let mut pos = 0;
    for (n, hunk) in hunks.iter().enumerate() {
        if let Some(anchor) = &hunk.anchor {
            let found = (pos..lines.len())
                .find(|&i| lines[i].trim() == anchor.trim())
                .ok_or_else(|| format!("hunk {}: anchor `{anchor}` not found", n + 1))?;
            pos = found + 1;
        }
        let start = if hunk.old.is_empty() {
            // Pure insertion: right after the anchor, or at the end without one.
            if hunk.anchor.is_some() {
                pos
            } else {
                lines.len()
            }
        } else {
            find_block(&lines, &hunk.old, pos).ok_or_else(|| {
                format!(
                    "hunk {} does not match the file (looked for `{}`)",
                    n + 1,
                    hunk.old.first().map(String::as_str).unwrap_or_default()
                )
            })?
        };
        lines.splice(start..start + hunk.old.len(), hunk.new.iter().cloned());
        pos = start + hunk.new.len();
    }
    let mut out = lines.join(eol);
    if had_newline || (content.is_empty() && !out.is_empty()) {
        out.push_str(eol);
    }
    Ok(out)
}

fn find_block(lines: &[String], block: &[String], from: usize) -> Option<usize> {
    if block.len() > lines.len() {
        return None;
    }
    let comparisons: [fn(&str, &str) -> bool; 3] = [
        |a, b| a == b,
        |a, b| a.trim_end() == b.trim_end(),
        |a, b| a.trim() == b.trim(),
    ];
    comparisons.iter().find_map(|same| {
        (from..=lines.len() - block.len())
            .find(|&i| block.iter().zip(&lines[i..]).all(|(b, l)| same(l, b)))
    })
}

/// What a patch does to one file once applied: its new content, or `None` when it is removed.
struct Change {
    path: PathBuf,
    content: Option<String>,
    /// The file before the patch, for undoing a partly written patch.
    original: Option<String>,
}

pub struct ApplyPatchTool {
    pub jail: PathJail,
}

impl ApplyPatchTool {
    pub fn with_jail(jail: PathJail) -> Self {
        Self { jail }
    }

    async fn plan(&self, ops: &[PatchOp]) -> Result<(Vec<Change>, Vec<String>), ToolError> {
        let fail = |message: String| ToolError::Execution(message);
        let mut changes = Vec::new();
        let mut summary = Vec::new();
        let mut seen = HashSet::new();
        for op in ops {
            let (PatchOp::Add { path, .. }
            | PatchOp::Delete { path }
            | PatchOp::Update { path, .. }) = op;
            let target = self.jail.validate_path(path)?;
            if !seen.insert(target.clone()) {
                return Err(fail(format!(
                    "`{path}` appears more than once in the patch"
                )));
            }
            let original = tokio::fs::read_to_string(&target).await.ok();
            match op {
                PatchOp::Add { content, .. } => {
                    if target.exists() {
                        return Err(fail(format!(
                            "`{path}` already exists; use `*** Update File:` to change it"
                        )));
                    }
                    changes.push(Change {
                        path: target,
                        content: Some(content.clone()),
                        original: None,
                    });
                    summary.push(format!("A {path}"));
                }
                PatchOp::Delete { .. } => {
                    if original.is_none() {
                        return Err(fail(format!("Cannot delete `{path}`: no such file")));
                    }
                    changes.push(Change {
                        path: target,
                        content: None,
                        original,
                    });
                    summary.push(format!("D {path}"));
                }
                PatchOp::Update { move_to, hunks, .. } => {
                    let Some(before) = original else {
                        return Err(fail(format!("Cannot update `{path}`: no such file")));
                    };
                    let after = apply_hunks(&before, hunks)
                        .map_err(|e| fail(format!("Cannot update `{path}`: {e}")))?;
                    match move_to {
                        Some(dest) => {
                            let dest_path = self.jail.validate_path(dest)?;
                            if dest_path.exists() || !seen.insert(dest_path.clone()) {
                                return Err(fail(format!("Cannot move to `{dest}`: it exists")));
                            }
                            changes.push(Change {
                                path: target,
                                content: None,
                                original: Some(before),
                            });
                            changes.push(Change {
                                path: dest_path,
                                content: Some(after),
                                original: None,
                            });
                            summary.push(format!("R {path} -> {dest}"));
                        }
                        None => {
                            changes.push(Change {
                                path: target,
                                content: Some(after),
                                original: Some(before),
                            });
                            summary.push(format!("M {path}"));
                        }
                    }
                }
            }
        }
        Ok((changes, summary))
    }
}

async fn write_change(change: &Change) -> std::io::Result<()> {
    match &change.content {
        Some(content) => {
            if let Some(parent) = change.path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&change.path, content).await
        }
        None => tokio::fs::remove_file(&change.path).await,
    }
}

/// Put a file back the way it was before `change` was written.
async fn undo_change(change: &Change) {
    let _ = match &change.original {
        Some(original) => tokio::fs::write(&change.path, original).await,
        None => tokio::fs::remove_file(&change.path).await,
    };
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Add, update, delete or move several files in one call. The patch starts with \
         `*** Begin Patch` and ends with `*** End Patch`. Each file starts with \
         `*** Add File: <path>` (every line prefixed with `+`), `*** Delete File: <path>`, or \
         `*** Update File: <path>` (optionally followed by `*** Move to: <path>`) and hunks: \
         an optional `@@ <line to find first>` anchor, then lines prefixed with ` ` (context), \
         `-` (remove) or `+` (add). Include about three lines of context around each change. \
         Hunks are checked before anything is written, so the patch applies completely or not \
         at all."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "The patch text, from `*** Begin Patch` to `*** End Patch`"
                }
            },
            "required": ["patch"]
        })
    }

    async fn execute(&self, args: serde_json::Value) -> Result<String, ToolError> {
        let patch = args["patch"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidParameters("`patch` must be a string".into()))?;
        let ops = parse_patch(patch).map_err(ToolError::InvalidParameters)?;
        let (changes, summary) = self.plan(&ops).await?;
        for (n, change) in changes.iter().enumerate() {
            if let Err(e) = write_change(change).await {
                for done in changes[..n].iter().rev() {
                    undo_change(done).await;
                }
                return Err(ToolError::Execution(format!(
                    "Failed to write {}: {e}. No file was changed.",
                    change.path.display()
                )));
            }
        }
        Ok(format!("Applied patch:\n{}", summary.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(body: &str) -> String {
        format!("{BEGIN}\n{body}\n{END}")
    }

    #[test]
    fn parses_every_operation() {
        let ops = parse_patch(&patch(
            "*** Add File: a.txt\n+one\n+two\n*** Update File: b.rs\n*** Move to: c.rs\n@@ fn x\n ctx\n-old\n+new\n\n*** Delete File: d.txt",
        ))
        .unwrap();
        assert_eq!(
            ops,
            [
                PatchOp::Add {
                    path: "a.txt".into(),
                    content: "one\ntwo\n".into()
                },
                PatchOp::Update {
                    path: "b.rs".into(),
                    move_to: Some("c.rs".into()),
                    hunks: vec![Hunk {
                        anchor: Some("fn x".into()),
                        old: vec!["ctx".into(), "old".into(), String::new()],
                        new: vec!["ctx".into(), "new".into(), String::new()],
                    }],
                },
                PatchOp::Delete {
                    path: "d.txt".into()
                },
            ]
        );
        assert_eq!(touched_paths(&ops), ["a.txt", "b.rs", "c.rs", "d.txt"]);
    }

    #[test]
    fn rejects_malformed_patches() {
        assert!(parse_patch("*** Add File: a\n+x")
            .unwrap_err()
            .contains("Begin Patch"));
        assert!(parse_patch(&patch("*** Add File: a\nx"))
            .unwrap_err()
            .contains("must start with `+`"));
        assert!(parse_patch(&patch("hello")).unwrap_err().contains("line 2"));
        assert!(parse_patch(&patch("")).is_err());
    }

    #[test]
    fn hunks_apply_in_order_after_anchors_and_tolerate_whitespace() {
        let content = "fn a() {\n    1\n}\nfn b() {\n    1\n}\n";
        let hunks = vec![
            Hunk {
                anchor: Some("fn b() {".into()),
                old: vec!["  1  ".into()],
                new: vec!["    2".into()],
            },
            Hunk {
                anchor: None,
                old: Vec::new(),
                new: vec!["fn c() {}".into()],
            },
        ];
        assert_eq!(
            apply_hunks(content, &hunks).unwrap(),
            "fn a() {\n    1\n}\nfn b() {\n    2\n}\nfn c() {}\n"
        );
        let missing = vec![Hunk {
            old: vec!["nope".into()],
            ..Hunk::default()
        }];
        assert!(apply_hunks(content, &missing)
            .unwrap_err()
            .contains("does not match"));
    }

    #[tokio::test]
    async fn applies_all_changes_or_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("keep.txt"), "a\nb\nc\n").unwrap();
        std::fs::write(root.join("gone.txt"), "x\n").unwrap();
        std::fs::write(root.join("old.txt"), "move me\n").unwrap();
        let tool = ApplyPatchTool::with_jail(PathJail::new(root));

        // The second update does not match: nothing is written.
        let bad =
            patch("*** Update File: keep.txt\n a\n-b\n+B\n*** Update File: old.txt\n-absent\n+x");
        assert!(tool.execute(json!({ "patch": bad })).await.is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "a\nb\nc\n"
        );

        let good = patch(
            "*** Update File: keep.txt\n a\n-b\n+B\n*** Delete File: gone.txt\n*** Add File: dir/new.txt\n+hi\n*** Update File: old.txt\n*** Move to: moved.txt\n-move me\n+moved",
        );
        let out = tool.execute(json!({ "patch": good })).await.unwrap();
        assert!(out.contains("M keep.txt") && out.contains("R old.txt -> moved.txt"));
        assert_eq!(
            std::fs::read_to_string(root.join("keep.txt")).unwrap(),
            "a\nB\nc\n"
        );
        assert!(!root.join("gone.txt").exists());
        assert!(!root.join("old.txt").exists());
        assert_eq!(
            std::fs::read_to_string(root.join("dir/new.txt")).unwrap(),
            "hi\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("moved.txt")).unwrap(),
            "moved\n"
        );
    }

    #[tokio::test]
    async fn stays_inside_the_workspace_and_does_not_clobber_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        let tool = ApplyPatchTool::with_jail(PathJail::new(dir.path()));
        let escape = patch("*** Add File: ../outside.txt\n+x");
        assert!(tool.execute(json!({ "patch": escape })).await.is_err());
        let clobber = patch("*** Add File: a.txt\n+x");
        let err = tool.execute(json!({ "patch": clobber })).await.unwrap_err();
        assert!(err.to_string().contains("already exists"));
    }
}
