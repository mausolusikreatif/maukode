---
name: safe-utf8-truncation-rust
description: "Truncate Rust strings safely on multi-byte UTF-8 boundaries to prevent panics with CJK/emoji content"
run_as_subagent: false
---
When truncating a Rust string to a byte length `n`, never use `&s[..n]` directly. Instead, use `let end = s.floor_char_boundary(n); &s[..end]` (stable since Rust 1.82). For environments without `floor_char_boundary`, use a manual loop: `let mut end = n; while end > 0 && !s.is_char_boundary(end) { end -= 1; } &s[..end]`. This prevents `byte index N is not a char boundary` panics when content contains multi-byte UTF-8 characters (CJK, emoji, accented letters). Always audit all string truncation sites in a codebase — grep for patterns like `&s[..N]`, `&s[N..]`, and `&str[..N]` to find unsafe slicing.
