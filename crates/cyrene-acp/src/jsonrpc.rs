//! JSON-RPC 2.0 message types for the ACP adapter (R27.1).
//!
//! The Agent Client Protocol is JSON-RPC over a transport (stdio or a socket).
//! This module models requests, responses, and the standard error codes so the
//! adapter can parse editor requests and return well-formed responses or scoped
//! errors (R27.5).

use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 request from the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Must be `"2.0"`.
    pub jsonrpc: String,
    /// The request id (echoed in the response). Notifications omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    /// The method name (e.g. `"prompt"`, `"initialize"`).
    pub method: String,
    /// The method parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

/// A JSON-RPC 2.0 response to the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// The id of the request this responds to.
    pub id: serde_json::Value,
    /// The result, present on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// The error, present on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// Builds a success response.
    #[must_use]
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Builds an error response.
    #[must_use]
    pub fn err(id: serde_json::Value, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_owned(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpcError {
    /// The error code (see the standard codes below).
    pub code: i32,
    /// A short error message.
    pub message: String,
}

impl RpcError {
    /// `-32700`: invalid JSON was received.
    #[must_use]
    pub fn parse_error() -> Self {
        Self {
            code: -32700,
            message: "Parse error".to_owned(),
        }
    }

    /// `-32600`: the request is not a valid JSON-RPC request.
    #[must_use]
    pub fn invalid_request(detail: &str) -> Self {
        Self {
            code: -32600,
            message: format!("Invalid Request: {detail}"),
        }
    }

    /// `-32601`: the method does not exist.
    #[must_use]
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
        }
    }

    /// `-32602`: invalid method parameters.
    #[must_use]
    pub fn invalid_params(detail: &str) -> Self {
        Self {
            code: -32602,
            message: format!("Invalid params: {detail}"),
        }
    }

    /// `-32603`: an internal error occurred.
    #[must_use]
    pub fn internal_error(detail: &str) -> Self {
        Self {
            code: -32603,
            message: format!("Internal error: {detail}"),
        }
    }
}

/// Parses a JSON-RPC request from raw bytes, validating the envelope.
///
/// # Errors
/// Returns an [`RpcError`] (parse or invalid-request) if the bytes are not a
/// valid JSON-RPC 2.0 request.
pub fn parse_request(raw: &str) -> Result<Request, RpcError> {
    let req: Request = serde_json::from_str(raw).map_err(|_| RpcError::parse_error())?;
    if req.jsonrpc != "2.0" {
        return Err(RpcError::invalid_request("jsonrpc must be \"2.0\""));
    }
    if req.method.is_empty() {
        return Err(RpcError::invalid_request("method must not be empty"));
    }
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_request() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"prompt","params":{"text":"hi"}}"#;
        let req = parse_request(raw).unwrap();
        assert_eq!(req.method, "prompt");
        assert_eq!(req.id, Some(json!(1)));
    }

    #[test]
    fn rejects_malformed_json() {
        let err = parse_request("{not json").unwrap_err();
        assert_eq!(err.code, -32700);
    }

    #[test]
    fn rejects_wrong_version() {
        let raw = r#"{"jsonrpc":"1.0","id":1,"method":"prompt"}"#;
        let err = parse_request(raw).unwrap_err();
        assert_eq!(err.code, -32600);
    }

    #[test]
    fn rejects_empty_method() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":""}"#;
        let err = parse_request(raw).unwrap_err();
        assert_eq!(err.code, -32600);
    }

    #[test]
    fn response_serializes_without_null_fields() {
        let resp = Response::ok(json!(1), json!({"text":"done"}));
        let s = serde_json::to_string(&resp).unwrap();
        assert!(s.contains("\"result\""));
        assert!(!s.contains("\"error\""));
    }

    #[test]
    fn error_response_round_trips() {
        let resp = Response::err(json!(2), RpcError::method_not_found("foo"));
        let s = serde_json::to_string(&resp).unwrap();
        let back: Response = serde_json::from_str(&s).unwrap();
        assert_eq!(resp, back);
        assert_eq!(back.error.unwrap().code, -32601);
    }
}
