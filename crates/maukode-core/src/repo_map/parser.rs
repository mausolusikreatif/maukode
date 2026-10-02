use std::collections::HashSet;
use std::path::Path;
use tree_sitter::{Node, Parser, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Struct,
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Method,
    TypeAlias,
}

impl SymbolKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::Function => "fn",
            Self::Method => "method",
            Self::TypeAlias => "type",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDef {
    pub name: String,
    pub kind: SymbolKind,
    pub signature: String,
    pub line: usize,
    pub parent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFileSymbols {
    pub definitions: Vec<SymbolDef>,
    pub references: HashSet<String>,
}

pub struct AstParser {
    rust_parser: Parser,
    ts_parser: Parser,
    tsx_parser: Parser,
    py_parser: Parser,
    go_parser: Parser,
}

impl AstParser {
    pub fn new() -> anyhow::Result<Self> {
        let mut rust_parser = Parser::new();
        rust_parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;

        let mut ts_parser = Parser::new();
        ts_parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;

        let mut tsx_parser = Parser::new();
        tsx_parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())?;

        let mut py_parser = Parser::new();
        py_parser.set_language(&tree_sitter_python::LANGUAGE.into())?;

        let mut go_parser = Parser::new();
        go_parser.set_language(&tree_sitter_go::LANGUAGE.into())?;

        Ok(Self {
            rust_parser,
            ts_parser,
            tsx_parser,
            py_parser,
            go_parser,
        })
    }

    pub fn parse_file(&mut self, path: &Path, content: &str) -> Option<ParsedFileSymbols> {
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        match ext {
            "rs" => {
                let tree = self.rust_parser.parse(content, None)?;
                Some(Self::extract_rust(&tree, content))
            }
            "ts" | "mts" | "cts" => {
                let tree = self.ts_parser.parse(content, None)?;
                Some(Self::extract_typescript(&tree, content))
            }
            "tsx" | "jsx" => {
                let tree = self.tsx_parser.parse(content, None)?;
                Some(Self::extract_typescript(&tree, content))
            }
            "js" | "mjs" | "cjs" => {
                let tree = self.ts_parser.parse(content, None)?;
                Some(Self::extract_typescript(&tree, content))
            }
            "py" => {
                let tree = self.py_parser.parse(content, None)?;
                Some(Self::extract_python(&tree, content))
            }
            "go" => {
                let tree = self.go_parser.parse(content, None)?;
                Some(Self::extract_go(&tree, content))
            }
            _ => None,
        }
    }

    // --- Rust AST Extraction ---
    fn extract_rust(tree: &Tree, source: &str) -> ParsedFileSymbols {
        let mut defs = Vec::new();
        let mut refs = HashSet::new();
        let bytes = source.as_bytes();

        Self::walk_rust_nodes(tree.root_node(), bytes, None, &mut defs, &mut refs);

        ParsedFileSymbols {
            definitions: defs,
            references: refs,
        }
    }

    fn walk_rust_nodes(
        node: Node,
        bytes: &[u8],
        parent: Option<&str>,
        defs: &mut Vec<SymbolDef>,
        refs: &mut HashSet<String>,
    ) {
        let kind = node.kind();

        match kind {
            "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Struct,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "enum_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Enum,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "trait_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Trait,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "type_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &[";"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::TypeAlias,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "function_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    let symbol_kind = if parent.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: symbol_kind,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "impl_item" => {
                // e.g. impl Foo or impl Bar for Foo
                let type_name = node
                    .child_by_field_name("type")
                    .map(|n| node_text(n, bytes))
                    .unwrap_or("");
                let trait_name = node
                    .child_by_field_name("trait")
                    .map(|n| node_text(n, bytes));

                let impl_label = if let Some(tr) = trait_name {
                    format!("impl {tr} for {type_name}")
                } else if !type_name.is_empty() {
                    format!("impl {type_name}")
                } else {
                    "impl".to_string()
                };

                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    Self::walk_rust_nodes(child, bytes, Some(&impl_label), defs, refs);
                }
                return;
            }
            "identifier" | "type_identifier" => {
                let text = node_text(node, bytes);
                if text.len() > 1 && !is_rust_keyword(text) {
                    refs.insert(text.to_string());
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_rust_nodes(child, bytes, parent, defs, refs);
        }
    }

    // --- TypeScript / JavaScript AST Extraction ---
    fn extract_typescript(tree: &Tree, source: &str) -> ParsedFileSymbols {
        let mut defs = Vec::new();
        let mut refs = HashSet::new();
        let bytes = source.as_bytes();

        Self::walk_ts_nodes(tree.root_node(), bytes, None, &mut defs, &mut refs);

        ParsedFileSymbols {
            definitions: defs,
            references: refs,
        }
    }

    fn walk_ts_nodes(
        node: Node,
        bytes: &[u8],
        parent: Option<&str>,
        defs: &mut Vec<SymbolDef>,
        refs: &mut HashSet<String>,
    ) {
        let kind = node.kind();

        match kind {
            "class_declaration" | "class" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        Self::walk_ts_nodes(child, bytes, Some(name), defs, refs);
                    }
                    return;
                }
            }
            "interface_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Interface,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "type_alias_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &[";"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::TypeAlias,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Method,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "identifier" | "type_identifier" => {
                let text = node_text(node, bytes);
                if text.len() > 1 && !is_js_keyword(text) {
                    refs.insert(text.to_string());
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_ts_nodes(child, bytes, parent, defs, refs);
        }
    }

    // --- Python AST Extraction ---
    fn extract_python(tree: &Tree, source: &str) -> ParsedFileSymbols {
        let mut defs = Vec::new();
        let mut refs = HashSet::new();
        let bytes = source.as_bytes();

        Self::walk_python_nodes(tree.root_node(), bytes, None, &mut defs, &mut refs);

        ParsedFileSymbols {
            definitions: defs,
            references: refs,
        }
    }

    fn walk_python_nodes(
        node: Node,
        bytes: &[u8],
        parent: Option<&str>,
        defs: &mut Vec<SymbolDef>,
        refs: &mut HashSet<String>,
    ) {
        let kind = node.kind();

        match kind {
            "class_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Class,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        Self::walk_python_nodes(child, bytes, Some(name), defs, refs);
                    }
                    return;
                }
            }
            "function_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    let symbol_kind = if parent.is_some() {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: symbol_kind,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: parent.map(|s| s.to_string()),
                    });
                }
            }
            "identifier" => {
                let text = node_text(node, bytes);
                if text.len() > 1 && !is_py_keyword(text) {
                    refs.insert(text.to_string());
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_python_nodes(child, bytes, parent, defs, refs);
        }
    }

    // --- Go AST Extraction ---
    fn extract_go(tree: &Tree, source: &str) -> ParsedFileSymbols {
        let mut defs = Vec::new();
        let mut refs = HashSet::new();
        let bytes = source.as_bytes();

        Self::walk_go_nodes(tree.root_node(), bytes, &mut defs, &mut refs);

        ParsedFileSymbols {
            definitions: defs,
            references: refs,
        }
    }

    fn walk_go_nodes(
        node: Node,
        bytes: &[u8],
        defs: &mut Vec<SymbolDef>,
        refs: &mut HashSet<String>,
    ) {
        let kind = node.kind();

        match kind {
            "type_declaration" => {
                let sig = node_text(node, bytes).lines().next().unwrap_or("").trim();
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "type_spec" {
                        if let Some(name_node) = child.child_by_field_name("name") {
                            let name = node_text(name_node, bytes);
                            defs.push(SymbolDef {
                                name: name.to_string(),
                                kind: SymbolKind::Struct,
                                signature: sig.to_string(),
                                line: node.start_position().row + 1,
                                parent: None,
                            });
                        }
                    }
                }
            }
            "function_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Function,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: None,
                    });
                }
            }
            "method_declaration" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    let name = node_text(name_node, bytes);
                    let receiver = node
                        .child_by_field_name("receiver")
                        .map(|n| node_text(n, bytes))
                        .unwrap_or("");
                    let sig = truncate_signature(node, bytes, &["body"]);
                    defs.push(SymbolDef {
                        name: name.to_string(),
                        kind: SymbolKind::Method,
                        signature: sig,
                        line: node.start_position().row + 1,
                        parent: if receiver.is_empty() {
                            None
                        } else {
                            Some(receiver.to_string())
                        },
                    });
                }
            }
            "identifier" | "type_identifier" => {
                let text = node_text(node, bytes);
                if text.len() > 1 && !is_go_keyword(text) {
                    refs.insert(text.to_string());
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            Self::walk_go_nodes(child, bytes, defs, refs);
        }
    }
}

fn node_text<'a>(node: Node, bytes: &'a [u8]) -> &'a str {
    std::str::from_utf8(&bytes[node.start_byte()..node.end_byte()]).unwrap_or("")
}

fn truncate_signature(node: Node, bytes: &[u8], body_field_names: &[&str]) -> String {
    for field in body_field_names {
        if let Some(body_node) = node.child_by_field_name(field) {
            let slice = &bytes[node.start_byte()..body_node.start_byte()];
            let s = std::str::from_utf8(slice).unwrap_or("").trim();
            if !s.is_empty() {
                return clean_signature(s);
            }
        }
    }

    let full = node_text(node, bytes);
    let first_line = full.lines().next().unwrap_or("").trim();
    if let Some(idx) = first_line.find('{') {
        clean_signature(&first_line[..idx])
    } else {
        clean_signature(first_line)
    }
}

fn clean_signature(sig: &str) -> String {
    sig.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break" | "const" | "continue" | "crate" | "else" | "enum" | "extern" | "false"
            | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move"
            | "mut" | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct" | "super"
            | "trait" | "true" | "type" | "unsafe" | "use" | "where" | "while" | "async" | "await"
            | "dyn"
    )
}

fn is_js_keyword(s: &str) -> bool {
    matches!(
        s,
        "break" | "case" | "catch" | "class" | "const" | "continue" | "debugger" | "default"
            | "delete" | "do" | "else" | "export" | "extends" | "finally" | "for" | "function"
            | "if" | "import" | "in" | "instanceof" | "new" | "return" | "super" | "switch"
            | "this" | "throw" | "try" | "typeof" | "var" | "void" | "while" | "with" | "yield"
            | "let" | "static" | "interface" | "type" | "async" | "await"
    )
}

fn is_py_keyword(s: &str) -> bool {
    matches!(
        s,
        "False" | "None" | "True" | "and" | "as" | "assert" | "async" | "await" | "break"
            | "class" | "continue" | "def" | "del" | "elif" | "else" | "except" | "finally"
            | "for" | "from" | "global" | "if" | "import" | "in" | "is" | "lambda" | "nonlocal"
            | "not" | "or" | "pass" | "raise" | "return" | "try" | "while" | "with" | "yield"
            | "self"
    )
}

fn is_go_keyword(s: &str) -> bool {
    matches!(
        s,
        "break" | "default" | "func" | "interface" | "select" | "case" | "defer" | "go" | "map"
            | "struct" | "chan" | "else" | "goto" | "package" | "switch" | "const" | "fallthrough"
            | "if" | "range" | "type" | "continue" | "for" | "import" | "return" | "var"
    )
}
