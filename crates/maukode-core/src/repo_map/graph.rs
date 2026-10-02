use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use super::parser::SymbolDef;

#[derive(Debug, Clone)]
pub struct FileSymbolData {
    pub relative_path: PathBuf,
    pub definitions: Vec<SymbolDef>,
    pub references: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct RankedFile {
    pub relative_path: PathBuf,
    pub score: f64,
    pub symbols: Vec<(SymbolDef, f64)>,
}

pub struct SymbolGraph;

impl SymbolGraph {
    /// Computes PageRank and relevance scores across all files and symbols
    pub fn rank_files(
        files: &[FileSymbolData],
        query: Option<&str>,
        focus_path: Option<&str>,
    ) -> Vec<RankedFile> {
        let n = files.len();
        if n == 0 {
            return Vec::new();
        }

        // 1. Index symbol definitions -> file indices
        let mut def_to_files: HashMap<String, Vec<usize>> = HashMap::new();
        for (idx, file) in files.iter().enumerate() {
            for def in &file.definitions {
                def_to_files
                    .entry(def.name.clone())
                    .or_default()
                    .push(idx);
            }
        }

        // 2. Build adjacency list: file_i -> set of target files it references
        let mut adj: Vec<HashSet<usize>> = vec![HashSet::new(); n];
        for (src_idx, file) in files.iter().enumerate() {
            for ref_name in &file.references {
                if let Some(target_files) = def_to_files.get(ref_name) {
                    for &target_idx in target_files {
                        if target_idx != src_idx {
                            adj[src_idx].insert(target_idx);
                        }
                    }
                }
            }
        }

        // 3. Compute PageRank
        // damping d = 0.85, 20 iterations
        let d = 0.85;
        let mut ranks = vec![1.0 / n as f64; n];

        for _ in 0..20 {
            let mut next_ranks = vec![(1.0 - d) / n as f64; n];
            for (i, targets) in adj.iter().enumerate() {
                if targets.is_empty() {
                    // Distribute evenly if dangling
                    let share = (d * ranks[i]) / n as f64;
                    for r in next_ranks.iter_mut() {
                        *r += share;
                    }
                } else {
                    let share = (d * ranks[i]) / targets.len() as f64;
                    for &t in targets {
                        next_ranks[t] += share;
                    }
                }
            }
            ranks = next_ranks;
        }

        // 4. Boost ranks based on query tokens or focus path
        let query_tokens: Vec<String> = query
            .map(|q| {
                q.split(|c: char| !c.is_alphanumeric())
                    .filter(|s| s.len() > 2)
                    .map(|s| s.to_lowercase())
                    .collect()
            })
            .unwrap_or_default();

        let mut ranked_files = Vec::with_capacity(n);

        for (idx, file) in files.iter().enumerate() {
            let mut file_score = ranks[idx];
            let path_str = file.relative_path.to_string_lossy().to_lowercase();

            // Boost if matches focus path
            if let Some(focus) = focus_path {
                if !focus.is_empty() && path_str.contains(&focus.to_lowercase()) {
                    file_score *= 2.5;
                }
            }

            // Rank individual symbols within the file
            let mut ranked_symbols = Vec::with_capacity(file.definitions.len());

            for def in &file.definitions {
                let mut sym_score = file_score;
                let name_lower = def.name.to_lowercase();

                // If symbol is referenced by multiple external files, boost
                let ref_count = files
                    .iter()
                    .enumerate()
                    .filter(|(f_idx, f)| *f_idx != idx && f.references.contains(&def.name))
                    .count();
                sym_score += (ref_count as f64) * 0.1;

                // Query match boost
                for token in &query_tokens {
                    if name_lower.contains(token) || path_str.contains(token) {
                        sym_score *= 3.0;
                        file_score *= 1.5;
                    }
                }

                ranked_symbols.push((def.clone(), sym_score));
            }

            // Sort symbols by line number or relevance
            ranked_symbols.sort_by(|a, b| a.0.line.cmp(&b.0.line));

            ranked_files.push(RankedFile {
                relative_path: file.relative_path.clone(),
                score: file_score,
                symbols: ranked_symbols,
            });
        }

        // Sort files by score descending
        ranked_files.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        ranked_files
    }
}
