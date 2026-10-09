//! Memory scopes: the boundary that every read and write is filtered on.

use std::path::{Path, PathBuf};

/// A memory scope key. Project scopes are the default (and safest); the global scope is a
/// deliberate, explicit opt-in for cross-project user preferences.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Scoped to a single repository / workspace root.
    Project(PathBuf),
    /// Cross-project user preferences.
    Global,
}

impl Scope {
    /// Build a project scope, canonicalizing the root when possible so that equivalent paths
    /// (symlinks, trailing slashes, relative vs absolute) collapse to one key.
    pub fn project(root: impl AsRef<Path>) -> Self {
        let canonical = root.as_ref().canonicalize().unwrap_or_else(|_| {
            let p = root.as_ref();
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir()
                    .map(|cwd| cwd.join(p))
                    .unwrap_or_else(|_| p.to_path_buf())
            }
        });
        Scope::Project(canonical)
    }

    pub fn global() -> Self {
        Scope::Global
    }

    /// The stable string persisted in the `scope` column.
    pub fn as_key(&self) -> String {
        match self {
            Scope::Project(path) => format!("project:{}", path.to_string_lossy()),
            Scope::Global => "global".to_string(),
        }
    }

    pub fn is_global(&self) -> bool {
        matches!(self, Scope::Global)
    }

    pub fn parse(key: &str) -> Option<Self> {
        if key == "global" {
            return Some(Scope::Global);
        }
        key.strip_prefix("project:")
            .map(|p| Scope::Project(PathBuf::from(p)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_round_trips() {
        let scope = Scope::global();
        assert_eq!(scope.as_key(), "global");
        assert_eq!(Scope::parse("global"), Some(Scope::Global));
    }

    #[test]
    fn project_key_is_prefixed() {
        let scope = Scope::project("/tmp/example");
        assert!(scope.as_key().starts_with("project:"));
        assert_eq!(
            Scope::parse(&scope.as_key()).filter(|s| !s.is_global()),
            Some(scope)
        );
    }
}
