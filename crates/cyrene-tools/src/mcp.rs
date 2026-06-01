//! MCP client: registers tools from configured MCP servers (R28.5, R28.6).
//!
//! An MCP_Server exposes additional tools over the Model Context Protocol. The
//! client connects to each configured server, lists its tools, and registers
//! them in the [`ToolRegistry`] alongside the built-ins — no core change
//! (R28.5). If a server is unavailable, the client records the failure and
//! continues with the remaining servers and tools (R28.6).
//!
//! The transport is abstracted behind [`McpConnection`] so the client is
//! testable without a live MCP server.

use cyrene_core::Risk;

use crate::error::ToolError;
use crate::tool::{Tool, ToolOutput, ToolRegistry};

/// A tool descriptor advertised by an MCP server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpToolDescriptor {
    /// The tool name (namespaced by the server, e.g. `"github.create_issue"`).
    pub name: String,
    /// The default risk for this tool's actions.
    pub risk: Risk,
}

/// A connection to one MCP server. Implemented by a real client in production
/// and a fake in tests.
pub trait McpConnection: Send + Sync {
    /// The server's configured name (for diagnostics).
    fn server_name(&self) -> &str;

    /// Lists the tools the server exposes.
    ///
    /// # Errors
    /// Returns an error string if the server is unreachable (R28.6).
    fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, String>;

    /// Invokes a tool on the server.
    ///
    /// # Errors
    /// Returns an error string on a transport or tool failure.
    fn invoke(&self, tool: &str, args: &serde_json::Value) -> Result<String, String>;
}

/// A [`Tool`] that proxies to an MCP server tool.
struct McpTool {
    descriptor: McpToolDescriptor,
    connection: std::sync::Arc<dyn McpConnection>,
}

impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.descriptor.name
    }
    fn default_risk(&self) -> Risk {
        self.descriptor.risk
    }
    fn run(&self, args: &serde_json::Value) -> Result<ToolOutput, ToolError> {
        match self.connection.invoke(&self.descriptor.name, args) {
            Ok(text) => Ok(ToolOutput::ok(text)),
            Err(e) => Err(ToolError::Execution(e)),
        }
    }
}

/// A record of an MCP server that failed to load, for ledger logging (R28.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpLoadFailure {
    /// The server name.
    pub server: String,
    /// Why it failed.
    pub reason: String,
}

/// Registers all tools from the given MCP connections into `registry`,
/// skipping (and recording) any server that is unavailable (R28.5, R28.6).
///
/// Returns the list of load failures so the caller can log them to the ledger.
pub fn register_mcp_servers(
    registry: &mut ToolRegistry,
    connections: Vec<std::sync::Arc<dyn McpConnection>>,
) -> Vec<McpLoadFailure> {
    let mut failures = Vec::new();

    for conn in connections {
        match conn.list_tools() {
            Ok(descriptors) => {
                for descriptor in descriptors {
                    registry.register(Box::new(McpTool {
                        descriptor,
                        connection: conn.clone(),
                    }));
                }
            }
            Err(reason) => {
                // R28.6: log the failure and continue with the rest.
                failures.push(McpLoadFailure {
                    server: conn.server_name().to_owned(),
                    reason,
                });
            }
        }
    }

    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use cyrene_core::ToolCall;
    use serde_json::json;
    use std::sync::Arc;

    /// A working MCP server fake exposing two tools.
    struct GoodServer;
    impl McpConnection for GoodServer {
        fn server_name(&self) -> &str {
            "github"
        }
        fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, String> {
            Ok(vec![
                McpToolDescriptor {
                    name: "github.list_issues".to_owned(),
                    risk: Risk::Low,
                },
                McpToolDescriptor {
                    name: "github.create_issue".to_owned(),
                    risk: Risk::Medium,
                },
            ])
        }
        fn invoke(&self, tool: &str, _args: &serde_json::Value) -> Result<String, String> {
            Ok(format!("{tool} result"))
        }
    }

    /// An unavailable MCP server fake.
    struct DownServer;
    impl McpConnection for DownServer {
        fn server_name(&self) -> &str {
            "linear"
        }
        fn list_tools(&self) -> Result<Vec<McpToolDescriptor>, String> {
            Err("connection refused".to_owned())
        }
        fn invoke(&self, _tool: &str, _args: &serde_json::Value) -> Result<String, String> {
            Err("connection refused".to_owned())
        }
    }

    #[test]
    fn registers_tools_from_available_server() {
        let mut registry = crate::builtins::builtin_registry();
        let before = registry.len();
        let failures = register_mcp_servers(&mut registry, vec![Arc::new(GoodServer)]);
        assert!(failures.is_empty());
        assert_eq!(registry.len(), before + 2);
        assert!(registry.contains("github.list_issues"));
        assert_eq!(registry.risk_of("github.create_issue"), Some(Risk::Medium));
    }

    #[test]
    fn dispatches_to_mcp_tool() {
        let mut registry = ToolRegistry::new();
        register_mcp_servers(&mut registry, vec![Arc::new(GoodServer)]);
        let (out, inv) = registry
            .dispatch(&ToolCall::new("github.list_issues", json!({})))
            .unwrap();
        assert_eq!(out.text, "github.list_issues result");
        assert_eq!(inv.tool, "github.list_issues");
    }

    #[test]
    fn unavailable_server_is_skipped_and_recorded() {
        let mut registry = crate::builtins::builtin_registry();
        let before = registry.len();
        let failures = register_mcp_servers(
            &mut registry,
            vec![Arc::new(DownServer), Arc::new(GoodServer)],
        );
        // The down server is recorded as a failure (R28.6)...
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].server, "linear");
        assert!(failures[0].reason.contains("connection refused"));
        // ...but the good server's tools still loaded.
        assert_eq!(registry.len(), before + 2);
        assert!(registry.contains("github.list_issues"));
    }
}
