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
//!
//! Besides the built-in table, filters can be added in `<project>/.maukode/filters.json` and in
//! `filters.json` next to the user's `config.json`; they are tried before the built-in ones, the
//! project's first. The file is `{"filters": [{"program": "terraform", "subcommands": ["plan"],
//! "strip": ["^Refreshing state"], "max_lines": 300}]}`: no `subcommands` means any, `strip` is a
//! list of regexes for lines to drop, and `max_lines` (0 = no limit) cuts the middle of a longer
//! result.

use crate::security::simple_command_words;
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Set to a value other than `0` to send raw output to the model.
const DISABLE_ENV: &str = "MAUKODE_NO_COMPRESS";

/// Name of the filter file, in the project's `.maukode` directory and in the user's config dir.
const FILTER_FILE: &str = "filters.json";

/// One built-in filter. Patterns and subcommands are whitespace/newline separated text so the
/// table stays easy to read and to extend.
struct Spec {
    program: &'static str,
    /// Subcommands the filter applies to, separated by spaces (empty = any).
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

const PYTEST_STRIP: &str = r"^=+ test session starts =+$
^platform\s
^cachedir:\s
^rootdir:\s
^configfile:\s
^plugins:\s
^collected \d+ items?$
^\S+\.py [.sxX]+\s*(\[\s*\d+%\])?$
^[.sxX]+\s*(\[\s*\d+%\])?$
^\S+::\S+ PASSED\s*(\[\s*\d+%\])?$";

const GO_STRIP: &str = r"^=== (RUN|PAUSE|CONT)\s
^\s*--- PASS:
^PASS$";

const PIP_STRIP: &str = r"^\s*(Collecting|Downloading|Using cached|Requirement already satisfied|Installing collected|Obtaining|Preparing metadata|Building wheel|Created wheel|Stored in directory)\b
^\s*(Attempting uninstall|Found existing installation|Uninstalling|Successfully uninstalled)\b";

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
    Spec {
        program: "git",
        subcommands: "log",
        strip: "",
        max_lines: 200,
    },
    Spec {
        program: "pytest",
        subcommands: "",
        strip: PYTEST_STRIP,
        max_lines: 400,
    },
    Spec {
        program: "go",
        subcommands: "test",
        strip: GO_STRIP,
        max_lines: 400,
    },
    Spec {
        program: "pip",
        subcommands: "install",
        strip: PIP_STRIP,
        max_lines: 0,
    },
    Spec {
        program: "pip3",
        subcommands: "install",
        strip: PIP_STRIP,
        max_lines: 0,
    },
];

struct Compiled {
    program: String,
    /// Empty matches any subcommand.
    subcommands: Vec<String>,
    strip: Vec<Regex>,
    max_lines: usize,
}

/// One entry of a `filters.json` file.
#[derive(Deserialize)]
struct UserFilter {
    program: String,
    #[serde(default)]
    subcommands: Vec<String>,
    #[serde(default)]
    strip: Vec<String>,
    #[serde(default)]
    max_lines: usize,
}

#[derive(Deserialize)]
struct UserFilterFile {
    #[serde(default)]
    filters: Vec<UserFilter>,
}

fn compile_patterns<'a>(patterns: impl Iterator<Item = &'a str>) -> Vec<Regex> {
    let mut strip = Vec::new();
    for pattern in patterns {
        // A pattern that does not compile only means that line is not filtered.
        if let Ok(regex) = Regex::new(pattern) {
            strip.push(regex);
        }
    }
    strip
}

fn builtin_filters() -> &'static [Compiled] {
    static FILTERS: OnceLock<Vec<Compiled>> = OnceLock::new();
    FILTERS.get_or_init(|| {
        let mut compiled = Vec::new();
        for spec in SPECS {
            let subcommands = spec.subcommands.split_whitespace();
            compiled.push(Compiled {
                program: spec.program.to_string(),
                subcommands: subcommands.map(String::from).collect(),
                strip: compile_patterns(spec.strip.lines()),
                max_lines: spec.max_lines,
            });
        }
        compiled
    })
}

fn parse_filters(json: &str) -> Result<Vec<Compiled>, serde_json::Error> {
    let file: UserFilterFile = serde_json::from_str(json)?;
    let mut compiled = Vec::new();
    for filter in file.filters {
        let patterns = filter.strip.iter().map(String::as_str);
        compiled.push(Compiled {
            program: filter.program,
            subcommands: filter.subcommands,
            strip: compile_patterns(patterns),
            max_lines: filter.max_lines,
        });
    }
    Ok(compiled)
}

/// The directory that holds the user's `config.json`, as `AppConfig::config_path` finds it.
fn user_config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("MAUKODE_HOME");
    let home = home.filter(|value| !value.is_empty());
    let base = match home {
        Some(home) => Some(PathBuf::from(home).join("config")),
        None => dirs::config_dir(),
    };
    base.map(|dir| dir.join("maukode"))
}

fn read_filter_file(path: &Path) -> Vec<Compiled> {
    let Ok(json) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match parse_filters(&json) {
        Ok(compiled) => compiled,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                "Ignoring invalid bash filter file: {error}"
            );
            Vec::new()
        }
    }
}

/// The filters from the project's and the user's `filters.json`, the project's first.
fn user_filters(project: &Path) -> Vec<Compiled> {
    let mut found = read_filter_file(&project.join(".maukode").join(FILTER_FILE));
    if let Some(dir) = user_config_dir() {
        found.extend(read_filter_file(&dir.join(FILTER_FILE)));
    }
    found
}

fn disabled() -> bool {
    match std::env::var_os(DISABLE_ENV) {
        Some(value) => !value.is_empty() && value != "0",
        None => false,
    }
}

/// The filter for `command`: a simple command whose program and subcommand have one. Leading
/// flags and a `+toolchain` before the subcommand (`cargo +nightly build`) are skipped. `extra`
/// is tried before the built-in table.
fn find_filter<'a>(command: &str, extra: &'a [Compiled]) -> Option<&'a Compiled> {
    let words = simple_command_words(command)?;
    let program = words.first()?.as_str();
    let subcommand = words
        .iter()
        .skip(1)
        .find(|word| !word.starts_with('-') && !word.starts_with('+'))
        .map(String::as_str);
    for filter in extra.iter().chain(builtin_filters()) {
        if filter.program != program {
            continue;
        }
        let wanted = &filter.subcommands;
        let any = wanted.is_empty();
        let listed = match subcommand {
            Some(sub) => wanted.iter().any(|name| name.as_str() == sub),
            None => false,
        };
        if any || listed {
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

fn compress_with(extra: &[Compiled], command: &str, exit_code: i32, text: &str) -> Option<String> {
    if exit_code != 0 || disabled() {
        return None;
    }
    let filter = find_filter(command, extra)?;
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

/// The shorter text for the output of `command` using the built-in filters, or `None` to use
/// `text` as it is.
pub fn compress_output(command: &str, exit_code: i32, text: &str) -> Option<String> {
    compress_with(&[], command, exit_code, text)
}

/// Like [`compress_output`], with the filters from `<project>/.maukode/filters.json` and the
/// user's `filters.json` tried first.
pub fn compress_output_in(
    project: &Path,
    command: &str,
    exit_code: i32,
    text: &str,
) -> Option<String> {
    if exit_code != 0 || disabled() {
        return None;
    }
    let extra = user_filters(project);
    compress_with(&extra, command, exit_code, text)
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

    #[test]
    fn pytest_progress_goes_and_the_summary_stays() {
        let text = "=== test session starts ===
platform linux -- Python 3.12.0
rootdir: /w
collected 3 items

tests/test_a.py ...                                     [100%]

=== 3 passed in 0.02s ===";
        let out = compress_output("pytest -q tests", 0, text).unwrap();
        assert!(!out.contains("platform"));
        assert!(!out.contains("collected"));
        assert!(!out.contains("test_a.py"));
        assert!(out.contains("3 passed in 0.02s"));
    }

    #[test]
    fn go_test_run_lines_go_and_the_package_result_stays() {
        let text = "=== RUN   TestA\n--- PASS: TestA (0.00s)\nPASS\nok  example.com/a  0.002s";
        let out = compress_output("go test ./...", 0, text).unwrap();
        assert_eq!(out, "ok  example.com/a  0.002s");
    }

    #[test]
    fn pip_install_progress_goes_and_the_result_stays() {
        let text = "Collecting x
  Downloading x-1.0.whl (10 kB)
Installing collected packages: x
Successfully installed x-1.0";
        for command in ["pip install x", "pip3 install x"] {
            let out = compress_output(command, 0, text).unwrap();
            assert_eq!(out, "Successfully installed x-1.0", "{command}");
        }
    }

    #[test]
    fn a_long_git_log_keeps_its_head_and_tail() {
        let lines: Vec<String> = (0..300).map(|i| format!("commit {i}")).collect();
        let out = compress_output("git log", 0, &lines.join("\n")).unwrap();
        assert!(out.contains("commit 0\n"));
        assert!(out.contains("commit 299"));
        assert!(out.contains("...[100 lines skipped]..."));
    }

    const TERRAFORM_FILTERS: &str =
        r#"{"filters":[{"program":"terraform","subcommands":["plan"],"strip":["^Refreshing"]}]}"#;
    const PLAN_OUTPUT: &str = "Refreshing state...\nPlan: 1 to add";

    #[test]
    fn a_filter_from_a_file_applies_to_its_subcommands_only() {
        let compiled = parse_filters(TERRAFORM_FILTERS).unwrap();
        let planned = compress_with(&compiled, "terraform plan", 0, PLAN_OUTPUT);
        assert_eq!(planned.as_deref(), Some("Plan: 1 to add"));
        assert_eq!(compress_with(&compiled, "terraform apply", 0, PLAN_OUTPUT), None);
        assert_eq!(compress_with(&compiled, "terraform plan", 1, PLAN_OUTPUT), None);
    }

    #[test]
    fn a_filter_from_a_file_wins_over_the_built_in_one() {
        const OVERRIDE: &str =
            r#"{"filters":[{"program":"cargo","subcommands":["build"],"strip":["^warning"]}]}"#;
        let compiled = parse_filters(OVERRIDE).unwrap();
        let out = compress_with(&compiled, "cargo build", 0, CARGO_BUILD).unwrap();
        assert!(out.contains("Compiling"), "the built-in filter is not applied");
        assert!(!out.contains("warning: unused"));
    }

    #[test]
    fn a_filter_without_subcommands_matches_any() {
        const ANY: &str = r#"{"filters":[{"program":"tool","strip":["^x"]}]}"#;
        let compiled = parse_filters(ANY).unwrap();
        assert!(compress_with(&compiled, "tool", 0, "x\ny").is_some());
        assert!(compress_with(&compiled, "tool run", 0, "x\ny").is_some());
    }

    #[test]
    fn an_invalid_filter_file_is_an_error() {
        assert!(parse_filters("{nope").is_err());
        assert!(parse_filters(r#"{"filters":[{"strip":[]}]}"#).is_err());
    }

    #[test]
    fn the_project_filter_file_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".maukode");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(config.join(FILTER_FILE), TERRAFORM_FILTERS).unwrap();
        let out = compress_output_in(dir.path(), "terraform plan", 0, PLAN_OUTPUT);
        assert_eq!(out.as_deref(), Some("Plan: 1 to add"));
    }
}
