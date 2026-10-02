use crate::tool::{Tool, ToolDefinition, ToolError};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default, Clone)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    aliases: HashMap<String, String>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            aliases: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    pub fn register_alias(&mut self, alias: impl Into<String>, target_name: impl Into<String>) {
        self.aliases.insert(alias.into(), target_name.into());
    }

    pub fn unregister(&mut self, name: &str) {
        self.tools.remove(name);
        self.aliases.retain(|k, v| k != name && v != name);
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        if let Some(tool) = self.tools.get(name) {
            return Some(Arc::clone(tool));
        }
        if let Some(target) = self.aliases.get(name) {
            return self.tools.get(target).cloned();
        }
        None
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub async fn execute(&self, name: &str, args: serde_json::Value) -> Result<String, ToolError> {
        let tool = self.get(name).ok_or_else(|| {
            let mut names: Vec<_> = self.tools.keys().map(String::as_str).collect();
            names.sort_unstable();
            ToolError::NotFound(format!(
                "Tool '{name}' is not registered. Available tools: {}. Call one of these exact names.",
                names.join(", ")
            ))
        })?;
        tool.execute(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTool;
    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            "test_tool"
        }
        fn description(&self) -> &str {
            "Test description"
        }
        fn parameters(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(&self, _args: serde_json::Value) -> Result<String, ToolError> {
            Ok("mock output".to_string())
        }
    }

    #[tokio::test]
    async fn test_tool_registry_aliases_and_unregister() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(MockTool));
        reg.register_alias("alias_tool", "test_tool");

        assert_eq!(reg.definitions().len(), 1);
        assert!(reg.get("test_tool").is_some());
        assert!(reg.get("alias_tool").is_some());

        let out = reg
            .execute("alias_tool", serde_json::json!({}))
            .await
            .unwrap();
        assert_eq!(out, "mock output");

        reg.unregister("test_tool");
        assert!(reg.get("test_tool").is_none());
        assert!(reg.get("alias_tool").is_none());
    }

    #[tokio::test]
    async fn unknown_name_returns_exact_available_names_without_execution() {
        let mut reg = ToolRegistry::new();
        reg.register(Arc::new(MockTool));

        let err = reg
            .execute("test_too", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotFound(message)
            if message.contains("test_too") && message.contains("test_tool")));
    }
}
