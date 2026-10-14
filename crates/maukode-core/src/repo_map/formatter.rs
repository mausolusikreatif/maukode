use super::graph::RankedFile;
use super::parser::SymbolKind;

pub struct RepoMapFormatter;

impl RepoMapFormatter {
    /// Formats ranked files and symbols into a concise codebase skeleton within a token budget.
    pub fn format(ranked_files: &[RankedFile], max_tokens: usize) -> String {
        if ranked_files.is_empty() {
            return String::new();
        }

        // Token estimation heuristic: ~4 characters per token
        let char_budget = max_tokens.saturating_mul(4);
        let mut output = String::new();

        // 1. First pass: render top ranked files with full signatures
        let mut remaining_files = Vec::new();

        for file in ranked_files {
            if file.symbols.is_empty() {
                continue;
            }

            let mut file_block = format!("{}:\n", file.relative_path.display());
            let mut methods_in_file = 0;

            for (sym, _) in &file.symbols {
                // If there are many methods, limit methods per file to keep breadth
                if sym.kind == SymbolKind::Method {
                    methods_in_file += 1;
                    if methods_in_file > 6 {
                        continue;
                    }
                }

                let sig = if sym.signature.len() > 80 {
                    let end = sym.signature.floor_char_boundary(77);
                    format!("{}...", &sym.signature[..end])
                } else {
                    sym.signature.clone()
                };

                let indent = if sym.parent.is_some() {
                    "    │   "
                } else {
                    "  │ "
                };

                file_block.push_str(&format!("{indent}{sig}\n"));
            }

            if output.len() + file_block.len() < char_budget {
                output.push_str(&file_block);
            } else {
                remaining_files.push(file);
            }
        }

        // 2. Second pass: for remaining files that didn't fit with full signatures,
        // summarize them in 1 line each (e.g. "path/to/file.rs: (struct Foo, fn bar)")
        if !remaining_files.is_empty() && output.len() < char_budget {
            output.push_str("\nOther modules:\n");
            for file in remaining_files {
                let sym_names: Vec<&str> = file
                    .symbols
                    .iter()
                    .take(4)
                    .map(|(s, _)| s.name.as_str())
                    .collect();

                let summary_line = format!(
                    "  • {}: {}\n",
                    file.relative_path.display(),
                    sym_names.join(", ")
                );

                if output.len() + summary_line.len() < char_budget {
                    output.push_str(&summary_line);
                } else {
                    break;
                }
            }
        }

        output.trim_end().to_string()
    }
}
