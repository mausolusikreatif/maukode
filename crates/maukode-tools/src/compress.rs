//! Shrinks the output of well-known commands before it reaches the model.
//!
//! Every token of a tool result is read again by each later request of the turn, so build
//! progress and per-test `ok` lines cost far more than their one appearance suggests.
//!
//! Rules that keep this safe:
//! - Only a run that exited with code 0 is touched; errors reach the model intact.
//! - Only one simple command is matched, with the parser the `bash_whitelist` uses
//!   ([`simple_command_words`]), so a chain, pipe or redirect is never filtered as if it were its
//!   first command.
//! - A filter only drops lines; nothing is rewritten. If it would leave nothing, or save nothing,
//!   the original text is used.
//! - `MAUKODE_NO_COMPRESS=1` turns it off, for when the raw output is wanted.

use crate::security::simple_command_words;
use regex::Regex;
use std::sync::OnceLock;

/// Set to a value other than `0` to send raw output to the model.
const DISABLE_ENV: &str = "MAUKODE_NO_COMPRESS";

/// One built-in filter. Patterns and subcommands are whitespace/newline separated text so the
/// table stays easy to read and to extend.
struct Spec {
    program: &'static str,
    /// Subcommands the filter applies to, separated by spaces.
    subcommands: &'static str,
    /// One regex per line; a line of output matching any of them is dropped.
    strip: &'static str,
    /// Longest result in lines (0 = no limit). The middle is cut when it is longer.
    max_lines: usize,
}

const CARGO_STRIP: &str = r"^\s*(Compiling|Checking|Fresh|Dirty|Blocking|Downloading|Downloaded|Updating|Locking|Adding|Fetching|Unpacking|Documenting)\s
^test .+ \.\.\. ok$
^running \d+ tests?$
^\s*Doc-tests\s";

const GIT_STRIP: &str = r#"^\s*\(use "git .*\)$
^\s*\(commit or discard the untracked or modified content in submodules\)$
^no changes added to commit
^(Enumerating|Counting|Compressing|Writing|Unpacking|Receiving|Resolving) (objects|deltas):
^Delta compression using
^Total \d+ \(delta
^remote: (Enumerating|Counting|Compressing|Total)\s"#;

const SPECS: &[Spec] = &[
    Spec {
        program: "cargo",
        subcommands: "build check test clippy doc",
        strip: CARGO_STRIP,
        max_lines: 400,
    },
    Spec {
        program: "git",
        subcommands: "status push pull fetch",
        strip: GIT_STRIP,
        max_lines: 0,
    },
];

struct Compiled {
    program: &'static str,
    subcommands: Vec<&'static str>,
    strip: Vec<Regex>,
    max_lines: usize,
}

fn filters() -> &'static [Compiled] {
    static FILTERS: OnceLock<Vec<Compiled>> = OnceLock::new();
    FILTERS.get_or_init(|| {
        let mut compiled = Vec::new();
        for spec in SPECS {
            let mut strip = Vec::new();
            for pattern in spec.strip.lines() {
                // A pattern that does not compile only means that line is not filtered.
                if let Ok(regex) = Regex::new(pattern) {
                    strip.push(regex);
                }
            }
            compiled.push(Compiled {
                program: spec.program,
                subcommands: spec.subcommands.split_whitespace().collect(),
                strip,
                max_lines: spec.max_lines,
            });
        }
        compiled
    })
}

fn disabled() -> bool {
    match std::env::var_os(DISABLE_ENV) {
        Some(value) => !value.is_empty() && value != "0",
        None => false,
    }
}

/// The filter for `command`: a simple command whose program and subcommand have one. Leading
/// flags and a `+toolchain` before the subcommand (`cargo +nightly build`) are skipped.
fn find_filter(command: &str) -> Option<&'static Compiled> {
    let words = simple_command_words(command)?;
    let program = words.first()?;
    let subcommand = words
        .iter()
        .skip(1)
        .find(|word| !word.starts_with('-') && !word.starts_with('+'))?;
    let subcommand = subcommand.as_str();
    for filter in filters() {
        let same_program = filter.program == program.as_str();
        let same_subcommand = filter.subcommands.iter().any(|s| *s == subcommand);
        if same_program && same_subcommand {
            return Some(filter);
        }
    }
    None
}

fn strip_ansi(text: &str) -> String {
    static ANSI: OnceLock<Option<Regex>> = OnceLock::new();
    let ansi = ANSI.get_or_init(|| Regex::new(r"\x1b\[[0-9;]*[A-Za-z]").ok());
    match ansi {
        Some(regex) => regex.replace_all(text, "").into_owned(),
        None => text.to_string(),
    }
}

/// Keep the first and last lines of an over-long result, with a marker for what was cut.
fn cap_lines(lines: Vec<&str>, max_lines: usize) -> Vec<String> {
    if max_lines == 0 || lines.len() <= max_lines {
        return lines.into_iter().map(String::from).collect();
    }
    let head = max_lines / 2;
    let tail = max_lines - head;
    let skipped = lines.len() - max_lines;
    let mut capped: Vec<String> = lines[..head].iter().map(|l| l.to_string()).collect();
    capped.push(format!("...[{skipped} lines skipped]..."));
    capped.extend(lines[lines.len() - tail..].iter().map(|l| l.to_string()));
    capped
}

/// The shorter text for the output of `command`, or `None` to use `text` as it is.
pub fn compress_output(command: &str, exit_code: i32, text: &str) -> Option<String> {
    if exit_code != 0 || disabled() {
        return None;
    }
    let filter = find_filter(command)?;
    let clean = strip_ansi(text);
    let mut kept: Vec<&str> = Vec::new();
    for line in clean.lines() {
        if !filter.strip.iter().any(|regex| regex.is_match(line)) {
            kept.push(line);
        }
    }
    if kept.is_empty() {
        return None;
    }
    let result = cap_lines(kept, filter.max_lines).join("\n");
    if result.len() >= text.len() {
        return None;
    }
    tracing::debug!(
        before = text.len(),
        after = result.len(),
        "Compressed bash output"
    );
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARGO_BUILD: &str = "[stderr]
   Compiling serde v1.0.0
   Compiling maukode-tools v0.5.0 (/w/crates/maukode-tools)
warning: unused variable: `x`
 --> src/lib.rs:3:9
    Finished `dev` profile in 4.2s";

    #[test]
    fn cargo_progress_goes_and_warnings_stay() {
        let out = compress_output("cargo build", 0, CARGO_BUILD).unwrap();
        assert!(!out.contains("Compiling"));
        assert!(out.contains("warning: unused variable"));
        assert!(out.contains("src/lib.rs:3:9"), "locations are kept");
        assert!(out.contains("Finished"));
        assert!(out.contains("[stderr]"));
    }

    #[test]
    fn passing_test_lines_go_and_the_summary_stays() {
        let text = "running 2 tests
test a::one ... ok
test a::two ... ok

test result: ok. 2 passed; 0 failed";
        let out = compress_output("cargo test --workspace", 0, text).unwrap();
        assert!(!out.contains("... ok"));
        assert!(out.contains("test result: ok. 2 passed"));
    }

    #[test]
    fn a_failed_run_is_never_touched() {
        assert_eq!(compress_output("cargo build", 101, CARGO_BUILD), None);
    }

    #[test]
    fn only_a_single_plain_command_is_matched() {
        for command in [
            "cargo build && rm -rf target",
            "cargo build | tee log",
            "cargo build > out.txt",
            "cd x; cargo build",
            "echo $(cargo build)",
            "cargo build \"unterminated",
        ] {
            assert_eq!(compress_output(command, 0, CARGO_BUILD), None, "{command}");
        }
    }

    #[test]
    fn commands_without_a_filter_are_left_alone() {
        assert_eq!(compress_output("ls -la", 0, CARGO_BUILD), None);
        assert_eq!(compress_output("cargo run", 0, CARGO_BUILD), None);
        assert_eq!(compress_output("git log", 0, CARGO_BUILD), None);
    }

    #[test]
    fn a_toolchain_and_flags_before_the_subcommand_are_skipped() {
        assert!(compress_output("cargo +nightly build", 0, CARGO_BUILD).is_some());
        assert!(compress_output("cargo -q build", 0, CARGO_BUILD).is_some());
    }

    #[test]
    fn git_status_hints_go_and_the_files_stay() {
        let text = "On branch main
Changes not staged for commit:
  (use \"git add <file>...\" to update what will be committed)
        modified:   src/lib.rs

no changes added to commit (use \"git add\" and/or \"git commit -a\")";
        let out = compress_output("git status", 0, text).unwrap();
        assert!(out.contains("On branch main"));
        assert!(out.contains("modified:   src/lib.rs"));
        assert!(!out.contains("(use \"git add"));
        assert!(!out.contains("no changes added"));
    }

    #[test]
    fn ansi_codes_are_removed_from_filtered_output() {
        let text = "\u{1b}[32m   Compiling\u{1b}[0m a v1\n\u{1b}[1mFinished\u{1b}[0m in 1s";
        let out = compress_output("cargo check", 0, text).unwrap();
        assert_eq!(out, "Finished in 1s");
    }

    #[test]
    fn nothing_left_or_nothing_saved_means_the_original() {
        assert_eq!(compress_output("cargo build", 0, "   Compiling a v1"), None);
        assert_eq!(compress_output("cargo build", 0, "Finished in 1s"), None);
    }

    #[test]
    fn a_long_result_keeps_its_head_and_tail() {
        let lines: Vec<String> = (0..600).map(|i| format!("warning {i}")).collect();
        let out = compress_output("cargo clippy", 0, &lines.join("\n")).unwrap();
        assert!(out.contains("warning 0"));
        assert!(out.contains("warning 599"));
        assert!(out.contains("...[200 lines skipped]..."));
        assert!(!out.contains("warning 300\n"));
    }
}
