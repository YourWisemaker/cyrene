//! The [`Tool`] trait and [`Tool_Registry`] (R28).
//!
//! A [`Tool`] is a discrete capability the Agent_Loop can invoke. Each tool
//! declares a default [`Risk`] so the autonomy policy and Approval_Gate apply
//! before a medium+/irreversible tool runs (R28.4). The [`ToolRegistry`]
//! registers tools by name and dispatches a [`ToolCall`] to the matching tool
//! (R28.2). Tool output is compressed before it enters model context (R28.3),
//! and every invocation is recorded in the ledger (R28.7) by the caller via the
//! returned [`ToolInvocation`] record.

use std::collections::BTreeMap;

use cyrene_core::{Risk, ToolCall};

use crate::error::ToolError;

/// The result of running a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    /// The tool's textual output (pre-compression).
    pub text: String,
    /// Whether the tool succeeded.
    pub success: bool,
}

impl ToolOutput {
    /// Creates a successful output.
    #[must_use]
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            success: true,
        }
    }

    /// Creates a failure output.
    #[must_use]
    pub fn failed(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            success: false,
        }
    }
}

/// A discrete capability the Agent_Loop can invoke.
pub trait Tool: Send + Sync {
    /// The stable tool name (e.g. `"fs.write"`, `"shell.run"`).
    fn name(&self) -> &str;

    /// The default risk this tool's actions carry, so the autonomy policy and
    /// Approval_Gate apply (R28.4).
    fn default_risk(&self) -> Risk;

    /// Executes the tool with the given arguments.
    ///
    /// # Errors
    /// Returns a [`ToolError`] if the arguments are invalid or execution fails.
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError>;
}

/// A record of a dispatched tool invocation, for ledger logging (R28.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolInvocation {
    /// The tool name.
    pub tool: String,
    /// The risk the tool's action carried.
    pub risk: Risk,
    /// Whether the invocation succeeded.
    pub success: bool,
}

/// The Tool_Registry: registers tools by name and dispatches calls (R28.2).
#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
        }
    }

    /// Registers a tool. Re-registering a name replaces the prior tool. This is
    /// how MCP tools and extension tools join the built-ins with no core change
    /// (R28.5).
    pub fn register(&mut self, tool: Box<dyn Tool>) {
        self.tools.insert(tool.name().to_owned(), tool);
    }

    /// Returns `true` if a tool with the given name is registered.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// The number of registered tools.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry has no tools.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// The names of all registered tools, sorted.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Looks up the default risk of a named tool, if registered.
    #[must_use]
    pub fn risk_of(&self, name: &str) -> Option<Risk> {
        self.tools.get(name).map(|t| t.default_risk())
    }

    /// Dispatches a [`ToolCall`] to the named tool and returns its output plus
    /// an invocation record for ledger logging (R28.2, R28.7).
    ///
    /// This does NOT apply the autonomy gate — the Agent_Loop calls
    /// [`ToolRegistry::risk_of`] and runs the gate before dispatching a
    /// medium+/irreversible tool (R28.4).
    ///
    /// # Errors
    /// Returns [`ToolError::NotFound`] if no tool matches, or the tool's own
    /// error.
    pub fn dispatch(&self, call: &ToolCall) -> Result<(ToolOutput, ToolInvocation), ToolError> {
        let tool = self
            .tools
            .get(&call.name)
            .ok_or_else(|| ToolError::NotFound(call.name.clone()))?;
        let output = tool.run(&call.args)?;
        let invocation = ToolInvocation {
            tool: call.name.clone(),
            risk: tool.default_risk(),
            success: output.success,
        };
        Ok((output, invocation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct EchoTool;
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn default_risk(&self) -> Risk {
            Risk::Low
        }
        fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
            let msg = args.get("msg").and_then(|v| v.as_str()).unwrap_or("");
            Ok(ToolOutput::ok(msg))
        }
    }

    struct DangerTool;
    impl Tool for DangerTool {
        fn name(&self) -> &str {
            "danger"
        }
        fn default_risk(&self) -> Risk {
            Risk::High
        }
        fn run(&self, _args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
            Ok(ToolOutput::ok("did something dangerous"))
        }
    }

    fn registry() -> ToolRegistry {
        let mut r = ToolRegistry::new();
        r.register(Box::new(EchoTool));
        r.register(Box::new(DangerTool));
        r
    }

    #[test]
    fn dispatch_routes_to_named_tool() {
        let r = registry();
        let call = ToolCall::new("echo", json!({ "msg": "hi" }));
        let (output, inv) = r.dispatch(&call).unwrap();
        assert_eq!(output.text, "hi");
        assert!(output.success);
        assert_eq!(inv.tool, "echo");
        assert_eq!(inv.risk, Risk::Low);
    }

    #[test]
    fn dispatch_unknown_tool_errors() {
        let r = registry();
        let call = ToolCall::new("nonexistent", json!({}));
        let err = r.dispatch(&call).unwrap_err();
        assert!(matches!(err, ToolError::NotFound(_)));
    }

    #[test]
    fn risk_of_reports_tool_risk() {
        let r = registry();
        assert_eq!(r.risk_of("echo"), Some(Risk::Low));
        assert_eq!(r.risk_of("danger"), Some(Risk::High));
        assert_eq!(r.risk_of("missing"), None);
    }

    #[test]
    fn register_replaces_existing_name() {
        let mut r = ToolRegistry::new();
        r.register(Box::new(EchoTool));
        assert_eq!(r.len(), 1);
        r.register(Box::new(EchoTool));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn names_lists_registered_tools() {
        let r = registry();
        assert_eq!(r.names(), vec!["danger".to_owned(), "echo".to_owned()]);
    }
}
