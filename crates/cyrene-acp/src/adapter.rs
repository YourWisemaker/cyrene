//! The ACP_Adapter: exposes the Agent_Loop to editors over JSON-RPC (R27).
//!
//! The adapter:
//!
//! - Runs editor prompts through the Agent_Loop and streams the response plus
//!   tool activity back to the editor (R27.1, R27.2).
//! - Presents proposed file edits to the editor for approval before they are
//!   applied, binding to the Approval_Gate (R27.3).
//! - Enforces the same autonomy/sandbox/approval model as other channels
//!   (R27.4) — the adapter never executes a gated edit without an explicit
//!   approve.
//! - Returns scoped JSON-RPC errors on malformed requests without affecting
//!   other in-flight requests (R27.5).
//!
//! The loop is abstracted behind the [`AcpBackend`] trait so the adapter is
//! testable without the full runtime.

use serde_json::json;

use crate::jsonrpc::{parse_request, Response, RpcError};

/// A unit of activity streamed back to the editor during a prompt (R27.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpEvent {
    /// A chunk of the assistant's response text.
    ResponseChunk(String),
    /// A tool invocation the loop performed (name + summary).
    ToolActivity { tool: String, summary: String },
    /// A proposed file edit awaiting editor approval (R27.3).
    EditProposed { path: String, summary: String },
    /// The final completion marker.
    Done,
}

/// An editor's decision on a proposed edit (R27.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditDecision {
    /// Apply the edit.
    Approve,
    /// Reject the edit (do not apply).
    Reject,
}

/// The backend the adapter drives: the Agent_Loop boundary.
pub trait AcpBackend {
    /// Runs a prompt and returns the stream of events (response + activity).
    /// Proposed edits appear as [`AcpEvent::EditProposed`] and are NOT applied
    /// until [`AcpBackend::resolve_edit`] approves them (R27.3, R27.4).
    fn run_prompt(&mut self, prompt: &str) -> Vec<AcpEvent>;

    /// Resolves a previously proposed edit. Returns `true` if the edit was
    /// applied (approved), `false` if rejected.
    fn resolve_edit(&mut self, path: &str, decision: EditDecision) -> bool;
}

/// The ACP adapter over a backend.
pub struct AcpAdapter<B> {
    backend: B,
}

impl<B: AcpBackend> AcpAdapter<B> {
    /// Creates an adapter over a backend.
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    /// Borrows the backend (for inspection in tests/wiring).
    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Handles one raw JSON-RPC request string, returning the response string.
    /// A malformed request yields a scoped error response and never panics or
    /// affects other requests (R27.5).
    pub fn handle(&mut self, raw: &str) -> String {
        let request = match parse_request(raw) {
            Ok(r) => r,
            Err(err) => {
                // No id available on a parse error; use null per JSON-RPC.
                return serialize(Response::err(json!(null), err));
            }
        };

        let id = request.id.clone().unwrap_or(json!(null));

        let response = match request.method.as_str() {
            "initialize" => Response::ok(
                id,
                json!({ "protocol": "acp", "version": "1", "capabilities": ["prompt", "edit"] }),
            ),
            "prompt" => self.handle_prompt(id, request.params),
            "resolve_edit" => self.handle_resolve_edit(id, request.params),
            other => Response::err(id, RpcError::method_not_found(other)),
        };

        serialize(response)
    }

    /// Handles a `prompt` request: runs it through the loop and returns the
    /// streamed events as the result (R27.2).
    fn handle_prompt(
        &mut self,
        id: serde_json::Value,
        params: Option<serde_json::Value>,
    ) -> Response {
        let Some(text) = params
            .as_ref()
            .and_then(|p| p.get("text"))
            .and_then(|t| t.as_str())
        else {
            return Response::err(id, RpcError::invalid_params("missing `text`"));
        };

        let events = self.backend.run_prompt(text);
        let serialized: Vec<serde_json::Value> = events.iter().map(event_to_json).collect();
        Response::ok(id, json!({ "events": serialized }))
    }

    /// Handles a `resolve_edit` request: binds the editor's decision to the
    /// approval gate (R27.3).
    fn handle_resolve_edit(
        &mut self,
        id: serde_json::Value,
        params: Option<serde_json::Value>,
    ) -> Response {
        let path = params
            .as_ref()
            .and_then(|p| p.get("path"))
            .and_then(|v| v.as_str());
        let approve = params
            .as_ref()
            .and_then(|p| p.get("approve"))
            .and_then(|v| v.as_bool());

        let (Some(path), Some(approve)) = (path, approve) else {
            return Response::err(id, RpcError::invalid_params("missing `path` or `approve`"));
        };

        let decision = if approve {
            EditDecision::Approve
        } else {
            EditDecision::Reject
        };
        let applied = self.backend.resolve_edit(path, decision);
        Response::ok(id, json!({ "applied": applied }))
    }
}

/// Serializes a response to a JSON string (infallible for our shapes).
fn serialize(resp: Response) -> String {
    serde_json::to_string(&resp).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"serialization failed"}}"#
            .to_owned()
    })
}

/// Converts an event into its JSON wire form.
fn event_to_json(event: &AcpEvent) -> serde_json::Value {
    match event {
        AcpEvent::ResponseChunk(text) => json!({ "type": "response", "text": text }),
        AcpEvent::ToolActivity { tool, summary } => {
            json!({ "type": "tool", "tool": tool, "summary": summary })
        }
        AcpEvent::EditProposed { path, summary } => {
            json!({ "type": "edit_proposed", "path": path, "summary": summary })
        }
        AcpEvent::Done => json!({ "type": "done" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A backend that emits a fixed event stream and records edit decisions.
    /// Edits are only "applied" when approved (R27.4).
    struct FakeBackend {
        applied_edits: RefCell<Vec<String>>,
        rejected_edits: RefCell<Vec<String>>,
    }
    impl FakeBackend {
        fn new() -> Self {
            Self {
                applied_edits: RefCell::new(Vec::new()),
                rejected_edits: RefCell::new(Vec::new()),
            }
        }
    }
    impl AcpBackend for FakeBackend {
        fn run_prompt(&mut self, prompt: &str) -> Vec<AcpEvent> {
            vec![
                AcpEvent::ResponseChunk(format!("Working on: {prompt}")),
                AcpEvent::ToolActivity {
                    tool: "fs.read".to_owned(),
                    summary: "read src/main.rs".to_owned(),
                },
                AcpEvent::EditProposed {
                    path: "src/main.rs".to_owned(),
                    summary: "add error handling".to_owned(),
                },
                AcpEvent::Done,
            ]
        }
        fn resolve_edit(&mut self, path: &str, decision: EditDecision) -> bool {
            match decision {
                EditDecision::Approve => {
                    self.applied_edits.borrow_mut().push(path.to_owned());
                    true
                }
                EditDecision::Reject => {
                    self.rejected_edits.borrow_mut().push(path.to_owned());
                    false
                }
            }
        }
    }

    fn adapter() -> AcpAdapter<FakeBackend> {
        AcpAdapter::new(FakeBackend::new())
    }

    #[test]
    fn initialize_returns_capabilities() {
        let mut a = adapter();
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let resp = a.handle(raw);
        assert!(resp.contains("\"protocol\":\"acp\""));
        assert!(resp.contains("prompt"));
        assert!(resp.contains("edit"));
    }

    #[test]
    fn prompt_streams_response_and_activity() {
        let mut a = adapter();
        let raw = r#"{"jsonrpc":"2.0","id":2,"method":"prompt","params":{"text":"fix the bug"}}"#;
        let resp = a.handle(raw);
        assert!(resp.contains("Working on: fix the bug"));
        assert!(resp.contains("\"type\":\"tool\""));
        assert!(resp.contains("\"type\":\"edit_proposed\""));
        assert!(resp.contains("\"type\":\"done\""));
    }

    #[test]
    fn proposed_edit_is_not_applied_until_approved() {
        let mut a = adapter();
        // Run a prompt that proposes an edit.
        a.handle(r#"{"jsonrpc":"2.0","id":1,"method":"prompt","params":{"text":"x"}}"#);
        // No edit applied yet (R27.3/27.4).
        assert!(a.backend().applied_edits.borrow().is_empty());

        // Editor approves the edit.
        let resp = a.handle(
            r#"{"jsonrpc":"2.0","id":2,"method":"resolve_edit","params":{"path":"src/main.rs","approve":true}}"#,
        );
        assert!(resp.contains("\"applied\":true"));
        assert_eq!(a.backend().applied_edits.borrow().len(), 1);
    }

    #[test]
    fn rejected_edit_is_not_applied() {
        let mut a = adapter();
        let resp = a.handle(
            r#"{"jsonrpc":"2.0","id":3,"method":"resolve_edit","params":{"path":"a.rs","approve":false}}"#,
        );
        assert!(resp.contains("\"applied\":false"));
        assert!(a.backend().applied_edits.borrow().is_empty());
        assert_eq!(a.backend().rejected_edits.borrow().len(), 1);
    }

    #[test]
    fn malformed_request_returns_scoped_error() {
        let mut a = adapter();
        let resp = a.handle("{garbage");
        assert!(resp.contains("-32700"));
        // The adapter is still usable for the next request (isolation, R27.5).
        let ok = a.handle(r#"{"jsonrpc":"2.0","id":9,"method":"initialize"}"#);
        assert!(ok.contains("\"protocol\":\"acp\""));
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let mut a = adapter();
        let resp = a.handle(r#"{"jsonrpc":"2.0","id":4,"method":"frobnicate"}"#);
        assert!(resp.contains("-32601"));
    }

    #[test]
    fn prompt_without_text_returns_invalid_params() {
        let mut a = adapter();
        let resp = a.handle(r#"{"jsonrpc":"2.0","id":5,"method":"prompt","params":{}}"#);
        assert!(resp.contains("-32602"));
    }

    #[test]
    fn resolve_edit_without_params_returns_invalid_params() {
        let mut a = adapter();
        let resp = a.handle(r#"{"jsonrpc":"2.0","id":6,"method":"resolve_edit","params":{}}"#);
        assert!(resp.contains("-32602"));
    }
}
