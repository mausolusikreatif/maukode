use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectedStack {
    pub name: String,
    pub language: String,
    pub build_system: String,
    pub package_manager: Option<String>,
    pub test_command: String,
    pub build_command: Option<String>,
    pub lint_command: Option<String>,
    pub format_command: Option<String>,
    pub frameworks: Vec<String>,
    pub description: String,
}

pub struct ProjectDetector;

impl ProjectDetector {
    /// Detect project stack and characteristics from the given directory
    pub async fn detect(dir: &Path) -> DetectedStack {
        // 1. Check Rust
        if dir.join("Cargo.toml").is_file() {
            return Self::detect_rust(dir).await;
        }

        // 2. Check Node / TypeScript / JavaScript
        if dir.join("package.json").is_file() {
            return Self::detect_node(dir).await;
        }

        // 3. Check Python
        if dir.join("pyproject.toml").is_file()
            || dir.join("requirements.txt").is_file()
            || dir.join("setup.py").is_file()
            || dir.join("Pipfile").is_file()
        {
            return Self::detect_python(dir).await;
        }

        // 4. Check Go
        if dir.join("go.mod").is_file() {
            return Self::detect_go(dir).await;
        }

        // 5. Check CMake / Makefile
        if dir.join("CMakeLists.txt").is_file() {
            let name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("project")
                .to_string();
            return DetectedStack {
                name,
                language: "C/C++".to_string(),
                build_system: "CMake".to_string(),
                package_manager: None,
                test_command: "ctest --test-dir build".to_string(),
                build_command: Some("cmake -B build && cmake --build build".to_string()),
                lint_command: Some("clang-tidy".to_string()),
                format_command: Some("clang-format -i".to_string()),
                frameworks: Vec::new(),
                description: "C/C++ project managed with CMake".to_string(),
            };
        }

        // Fallback: Generic project
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();
        DetectedStack {
            name,
            language: "Polyglot / Generic".to_string(),
            build_system: "Custom".to_string(),
            package_manager: None,
            test_command: "# Add test command here".to_string(),
            build_command: None,
            lint_command: None,
            format_command: None,
            frameworks: Vec::new(),
            description: "General software project".to_string(),
        }
    }

    async fn detect_rust(dir: &Path) -> DetectedStack {
        let cargo_path = dir.join("Cargo.toml");
        let mut name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rust-project")
            .to_string();
        let mut is_workspace = false;
        let mut frameworks = Vec::new();

        if let Ok(content) = tokio::fs::read_to_string(&cargo_path).await {
            if content.contains("[workspace]") {
                is_workspace = true;
            }
            if let Some(pkg_name) = extract_toml_value(&content, "name") {
                name = pkg_name;
            }
            if content.contains("tokio") {
                frameworks.push("tokio".to_string());
            }
            if content.contains("axum") {
                frameworks.push("axum".to_string());
            }
            if content.contains("actix-web") {
                frameworks.push("actix-web".to_string());
            }
            if content.contains("serde") {
                frameworks.push("serde".to_string());
            }
            if content.contains("ratatui") {
                frameworks.push("ratatui".to_string());
            }
        }

        let desc = if is_workspace {
            format!("Rust multi-crate workspace ({name})")
        } else {
            format!("Rust package ({name})")
        };

        DetectedStack {
            name,
            language: "Rust".to_string(),
            build_system: "Cargo".to_string(),
            package_manager: Some("cargo".to_string()),
            test_command: if is_workspace {
                "cargo test --workspace".to_string()
            } else {
                "cargo test".to_string()
            },
            build_command: Some("cargo build".to_string()),
            lint_command: Some(
                "cargo clippy --all-targets --all-features -- -D warnings".to_string(),
            ),
            format_command: Some("cargo fmt --all".to_string()),
            frameworks,
            description: desc,
        }
    }

    async fn detect_node(dir: &Path) -> DetectedStack {
        let pkg_path = dir.join("package.json");
        let mut name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("node-project")
            .to_string();
        let mut frameworks = Vec::new();
        let mut test_cmd = "npm test".to_string();
        let mut build_cmd = None;
        let mut lint_cmd = None;
        let is_ts = dir.join("tsconfig.json").is_file();

        let pkg_mgr = if dir.join("pnpm-lock.yaml").is_file() {
            "pnpm"
        } else if dir.join("yarn.lock").is_file() {
            "yarn"
        } else if dir.join("bun.lockb").is_file() || dir.join("bun.lock").is_file() {
            "bun"
        } else {
            "npm"
        };

        if let Ok(content) = tokio::fs::read_to_string(&pkg_path).await {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(n) = json.get("name").and_then(|v| v.as_str()) {
                    name = n.to_string();
                }
                if let Some(scripts) = json.get("scripts").and_then(|v| v.as_object()) {
                    if scripts.contains_key("test") {
                        test_cmd = format!("{pkg_mgr} test");
                    }
                    if scripts.contains_key("build") {
                        build_cmd = Some(format!("{pkg_mgr} run build"));
                    }
                    if scripts.contains_key("lint") {
                        lint_cmd = Some(format!("{pkg_mgr} run lint"));
                    }
                }
                let mut check_dep = |dep_name: &str| {
                    let has_dep = json
                        .get("dependencies")
                        .and_then(|d| d.get(dep_name))
                        .is_some()
                        || json
                            .get("devDependencies")
                            .and_then(|d| d.get(dep_name))
                            .is_some();
                    if has_dep {
                        frameworks.push(dep_name.to_string());
                    }
                };
                check_dep("react");
                check_dep("next");
                check_dep("vue");
                check_dep("svelte");
                check_dep("express");
                check_dep("@nestjs/core");
                check_dep("vite");
                check_dep("tailwindcss");
            }
        }

        let lang = if is_ts { "TypeScript" } else { "JavaScript" };

        DetectedStack {
            name: name.clone(),
            language: lang.to_string(),
            build_system: pkg_mgr.to_string(),
            package_manager: Some(pkg_mgr.to_string()),
            test_command: test_cmd,
            build_command: build_cmd,
            lint_command: lint_cmd,
            format_command: Some(format!("{pkg_mgr} run format")),
            frameworks,
            description: format!("{lang} project ({name}) using {pkg_mgr}"),
        }
    }

    async fn detect_python(dir: &Path) -> DetectedStack {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("python-project")
            .to_string();
        let mut frameworks = Vec::new();
        let mut test_cmd = "pytest".to_string();
        let mut lint_cmd = Some("ruff check .".to_string());
        let format_cmd = Some("ruff format .".to_string());

        let pkg_mgr = if dir.join("poetry.lock").is_file() {
            test_cmd = "poetry run pytest".to_string();
            lint_cmd = Some("poetry run ruff check .".to_string());
            "poetry"
        } else if dir.join("uv.lock").is_file() {
            test_cmd = "uv run pytest".to_string();
            lint_cmd = Some("uv run ruff check .".to_string());
            "uv"
        } else {
            "pip"
        };

        if let Ok(pyproject) = tokio::fs::read_to_string(dir.join("pyproject.toml")).await {
            if pyproject.contains("fastapi") {
                frameworks.push("FastAPI".to_string());
            }
            if pyproject.contains("django") {
                frameworks.push("Django".to_string());
            }
            if pyproject.contains("flask") {
                frameworks.push("Flask".to_string());
            }
            if pyproject.contains("torch") {
                frameworks.push("PyTorch".to_string());
            }
        }

        DetectedStack {
            name,
            language: "Python".to_string(),
            build_system: pkg_mgr.to_string(),
            package_manager: Some(pkg_mgr.to_string()),
            test_command: test_cmd,
            build_command: None,
            lint_command: lint_cmd,
            format_command: format_cmd,
            frameworks,
            description: format!("Python project managed with {pkg_mgr}"),
        }
    }

    async fn detect_go(dir: &Path) -> DetectedStack {
        let mut name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("go-project")
            .to_string();
        if let Ok(content) = tokio::fs::read_to_string(dir.join("go.mod")).await {
            if let Some(line) = content.lines().find(|l| l.starts_with("module ")) {
                let mod_name = line.trim_start_matches("module ").trim();
                if !mod_name.is_empty() {
                    name = mod_name.to_string();
                }
            }
        }

        DetectedStack {
            name,
            language: "Go".to_string(),
            build_system: "go".to_string(),
            package_manager: Some("go".to_string()),
            test_command: "go test -v ./...".to_string(),
            build_command: Some("go build ./...".to_string()),
            lint_command: Some("golangci-lint run".to_string()),
            format_command: Some("go fmt ./...".to_string()),
            frameworks: Vec::new(),
            description: "Go module project".to_string(),
        }
    }

    /// Generate idiomatic AGENTS.md content based on detected stack
    pub fn generate_agents_md(stack: &DetectedStack, dir: &Path) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Project: {}\n\n", stack.name));
        md.push_str(&format!("{}\n\n", stack.description));

        md.push_str("## Tech Stack\n");
        md.push_str(&format!("- **Language**: {}\n", stack.language));
        md.push_str(&format!("- **Build System**: {}\n", stack.build_system));
        if let Some(ref pm) = stack.package_manager {
            md.push_str(&format!("- **Package Manager**: {}\n", pm));
        }
        if !stack.frameworks.is_empty() {
            md.push_str(&format!(
                "- **Key Frameworks / Libraries**: {}\n",
                stack.frameworks.join(", ")
            ));
        }
        md.push('\n');

        md.push_str("## Commands\n\n");
        if let Some(ref b) = stack.build_command {
            md.push_str("### Build\n```bash\n");
            md.push_str(b);
            md.push_str("\n```\n\n");
        }

        md.push_str("### Test\n```bash\n");
        md.push_str(&stack.test_command);
        md.push_str("\n```\n\n");

        if stack.lint_command.is_some() || stack.format_command.is_some() {
            md.push_str("### Lint & Format\n```bash\n");
            if let Some(ref l) = stack.lint_command {
                md.push_str(l);
                md.push('\n');
            }
            if let Some(ref f) = stack.format_command {
                md.push_str(f);
                md.push('\n');
            }
            md.push_str("```\n\n");
        }

        md.push_str("## Code Style & Conventions\n");
        match stack.language.as_str() {
            "Rust" => {
                md.push_str("- **Idiomatic Error Handling**: Use `Result<T, E>` with `thiserror` or `anyhow`; avoid `unwrap()` or `expect()` in production paths.\n");
                md.push_str("- **Formatting**: Follow standard `rustfmt` guidelines; keep line length <= 100 where reasonable.\n");
                md.push_str(
                    "- **Async Runtime**: Prefer `tokio` for async tasks and non-blocking I/O.\n",
                );
                md.push_str("- **Clippy**: Ensure code passes `cargo clippy --all-targets` without warnings.\n");
            }
            "TypeScript" | "JavaScript" => {
                md.push_str("- **Type Safety**: Strictly avoid `any` in TypeScript; use explicit return types for public functions.\n");
                md.push_str("- **Modern Syntax**: Prefer ES modules (`import`/`export`), async/await over raw Promises, and immutable patterns.\n");
                md.push_str("- **Code Style**: Follow ESLint and Prettier rules configured in the project.\n");
            }
            "Python" => {
                md.push_str(
                    "- **PEP 8 & Type Hints**: Use Python 3.10+ type annotations throughout.\n",
                );
                md.push_str(
                    "- **Modern Tooling**: Prefer `ruff` for ultra-fast linting and formatting.\n",
                );
                md.push_str("- **Docstrings**: Provide clear Google-style or NumPy-style docstrings for public modules and functions.\n");
            }
            "Go" => {
                md.push_str("- **Error Handling**: Always check errors explicitly (`if err != nil`); do not discard errors with `_`.\n");
                md.push_str("- **Simplicity**: Follow standard Go idioms; keep packages cohesive and small.\n");
                md.push_str("- **Formatting**: Always format with `gofmt` or `goimports`.\n");
            }
            _ => {
                md.push_str("- Follow existing naming, formatting, and structural patterns in the codebase.\n");
                md.push_str(
                    "- Write clear, maintainable code with unit tests for new functionality.\n",
                );
            }
        }
        md.push('\n');

        // Discover top-level directories
        let mut top_dirs = Vec::new();
        if let Ok(mut entries) = std::fs::read_dir(dir) {
            while let Some(Ok(entry)) = entries.next() {
                if let Ok(ft) = entry.file_type() {
                    if ft.is_dir() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if !name.starts_with('.')
                            && name != "target"
                            && name != "node_modules"
                            && name != "dist"
                            && name != "build"
                        {
                            top_dirs.push(name);
                        }
                    }
                }
            }
        }
        top_dirs.sort();

        if !top_dirs.is_empty() {
            md.push_str("## Key Directories\n");
            for d in top_dirs {
                md.push_str(&format!("- `{d}/`: Core project components\n"));
            }
            md.push('\n');
        }

        md.push_str("## Agent Guidelines\n");
        md.push_str("- **Inspect Before Modifying**: Read existing code to understand architecture and conventions before editing.\n");
        md.push_str("- **Surgical Edits**: Use surgical search-and-replace rather than overwriting entire files whenever possible.\n");
        md.push_str("- **Verify Changes**: Always execute the test suite (`");
        md.push_str(&stack.test_command);
        md.push_str("`) before declaring a task complete.\n");
        md.push_str("- **Documentation Integrity**: Keep comments, docstrings, and documentation up to date with code changes.\n");

        md
    }
}

fn extract_toml_value(toml: &str, key: &str) -> Option<String> {
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key) {
            if let Some((k, v)) = trimmed.split_once('=') {
                if k.trim() == key {
                    let cleaned = v.trim().trim_matches('"').trim_matches('\'').trim();
                    return Some(cleaned.to_string());
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitResult {
    pub path: PathBuf,
    pub created: bool,
    pub overwritten: bool,
    pub detected_type: String,
    pub message: String,
    pub content: String,
}

/// Initialize standard AGENTS.md in the target directory
pub async fn init_project(dir: &Path, force: bool) -> anyhow::Result<InitResult> {
    let target_file = dir.join("AGENTS.md");
    let stack = ProjectDetector::detect(dir).await;
    let content = ProjectDetector::generate_agents_md(&stack, dir);

    let exists = target_file.exists();
    if exists && !force {
        return Ok(InitResult {
            path: target_file,
            created: false,
            overwritten: false,
            detected_type: stack.description,
            message: "Standard AGENTS.md already exists. Use --force to overwrite.".to_string(),
            content,
        });
    }

    tokio::fs::write(&target_file, &content).await?;

    let message = if exists {
        format!("✓ Overwrote standard AGENTS.md ({})", stack.description)
    } else {
        format!("✓ Generated standard AGENTS.md ({})", stack.description)
    };

    Ok(InitResult {
        path: target_file,
        created: !exists,
        overwritten: exists,
        detected_type: stack.description,
        message,
        content,
    })
}
