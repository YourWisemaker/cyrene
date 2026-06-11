//! WhatsApp Cloud API bridge.
//!
//! Unlike Telegram, WhatsApp's Cloud API has no long-poll: Meta delivers
//! messages by calling *your* webhook. So this bridge runs a tiny built-in
//! HTTP server (std `TcpListener`, no extra dependencies) that:
//!
//! - answers the one-time verification handshake (`GET` with
//!   `hub.mode=subscribe`), and
//! - receives message webhooks (`POST`), generating a reply with the configured
//!   model and sending it back through the Graph API.
//!
//! To use it you need, in `~/.cyrene/.env`:
//!   WHATSAPP_TOKEN            — a Cloud API access token
//!   WHATSAPP_PHONE_NUMBER_ID  — the sending phone-number id
//!   WHATSAPP_VERIFY_TOKEN     — any secret you also paste into Meta's webhook UI
//! and a public URL pointing at this server (e.g. an ngrok/cloudflared tunnel).
//!
//! The HTTP parsing here is intentionally minimal — enough for Meta's webhook
//! shape — and the model/token plumbing mirrors `telegram.rs` so both channels
//! feel the same.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use cyrene_core::{ChatMessage, Model};
use serde_json::Value;

use crate::agent;

/// Credentials and settings for the bridge, read from the environment.
pub struct Settings {
    pub token: String,
    pub phone_number_id: String,
    pub verify_token: String,
    pub port: u16,
}

impl Settings {
    /// Loads settings from the environment, reporting the first missing key.
    pub fn from_env() -> Result<Self, String> {
        let token = req_env("WHATSAPP_TOKEN")?;
        let phone_number_id = req_env("WHATSAPP_PHONE_NUMBER_ID")?;
        let verify_token = req_env("WHATSAPP_VERIFY_TOKEN")?;
        let port = std::env::var("WHATSAPP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8088);
        Ok(Self {
            token,
            phone_number_id,
            verify_token,
            port,
        })
    }
}

fn req_env(key: &str) -> Result<String, String> {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => Err(format!("{key} is not set in ~/.cyrene/.env")),
    }
}

/// A parsed incoming message: who sent it and the text body.
#[derive(Debug, PartialEq)]
pub struct Incoming {
    pub from: String,
    pub text: String,
}

/// Extracts text messages from a WhatsApp webhook payload. The shape is
/// `entry[].changes[].value.messages[]` with `from` and `text.body`.
#[must_use]
pub fn parse_incoming(payload: &Value) -> Vec<Incoming> {
    let mut out = Vec::new();
    let Some(entries) = payload["entry"].as_array() else {
        return out;
    };
    for entry in entries {
        let Some(changes) = entry["changes"].as_array() else {
            continue;
        };
        for change in changes {
            let Some(messages) = change["value"]["messages"].as_array() else {
                continue;
            };
            for m in messages {
                let from = m["from"].as_str().unwrap_or_default();
                let text = m["text"]["body"].as_str().unwrap_or_default();
                if !from.is_empty() && !text.is_empty() {
                    out.push(Incoming {
                        from: from.to_owned(),
                        text: text.to_owned(),
                    });
                }
            }
        }
    }
    out
}

/// Sends a text message via the Graph API.
async fn send_message(client: &reqwest::Client, settings: &Settings, to: &str, text: &str) {
    let url = format!(
        "https://graph.facebook.com/v20.0/{}/messages",
        settings.phone_number_id
    );
    let body = serde_json::json!({
        "messaging_product": "whatsapp",
        "to": to,
        "type": "text",
        "text": { "body": text },
    });
    if let Err(e) = client
        .post(&url)
        .bearer_auth(&settings.token)
        .json(&body)
        .send()
        .await
    {
        eprintln!("  whatsapp: send error: {e}");
    }
}

/// A minimally-parsed HTTP request: method, query params, and body. (The path
/// is ignored — every route is treated as the webhook endpoint.)
struct HttpRequest {
    method: String,
    query: HashMap<String, String>,
    body: String,
}

/// Reads and parses one HTTP/1.1 request from `stream`. Returns `None` on a
/// malformed or empty connection.
fn read_request(stream: &mut TcpStream) -> Option<HttpRequest> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    // Read until we have the full header block.
    let header_end = loop {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        if buf.len() > 64 * 1024 {
            return None; // runaway header
        }
    };

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();

    let mut content_length = 0usize;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }

    let (_path, query) = split_target(&target);

    // Read the remaining body up to content_length.
    let mut body = buf[header_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut tmp).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(content_length);

    Some(HttpRequest {
        method,
        query,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

/// Splits a request target into a path and decoded query map.
fn split_target(target: &str) -> (String, HashMap<String, String>) {
    let mut query = HashMap::new();
    let (path, qs) = match target.split_once('?') {
        Some((p, q)) => (p.to_owned(), q),
        None => (target.to_owned(), ""),
    };
    for pair in qs.split('&').filter(|s| !s.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        query.insert(url_decode(k), url_decode(v));
    }
    (path, query)
}

/// Minimal percent-decoding (enough for webhook verification params).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.flush();
}

/// Verifies the webhook handshake, returning the challenge to echo on success.
#[must_use]
pub fn verify_challenge(query: &HashMap<String, String>, expected_token: &str) -> Option<String> {
    let mode = query.get("hub.mode").map(String::as_str);
    let token = query.get("hub.verify_token").map(String::as_str);
    if mode == Some("subscribe") && token == Some(expected_token) {
        query.get("hub.challenge").cloned()
    } else {
        None
    }
}

/// Runs the bridge: starts the webhook server and answers messages until
/// interrupted. Blocking; uses the provided runtime for async HTTP/model calls.
pub fn run(rt: &tokio::runtime::Runtime, model: Arc<dyn Model>, settings: Settings) {
    let listener = match TcpListener::bind(("0.0.0.0", settings.port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("  ✗ Could not bind port {}: {e}", settings.port);
            return;
        }
    };
    println!(
        "✓ WhatsApp webhook listening on http://0.0.0.0:{} (Ctrl-C to stop)",
        settings.port
    );
    println!(
        "  Point your Meta webhook (and verify token) at a public URL forwarding to this port."
    );

    // Auto-start the scheduler so scheduled reports are delivered while the
    // webhook server runs.
    crate::crons::spawn_background();

    let client = reqwest::Client::new();
    let mut histories: HashMap<String, Vec<ChatMessage>> = HashMap::new();

    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        let Some(req) = read_request(&mut stream) else {
            write_response(&mut stream, "400 Bad Request", "bad request");
            continue;
        };

        // Verification handshake (GET) — echo the challenge when the token matches.
        if req.method == "GET" {
            match verify_challenge(&req.query, &settings.verify_token) {
                Some(challenge) => write_response(&mut stream, "200 OK", &challenge),
                None => write_response(&mut stream, "403 Forbidden", "verification failed"),
            }
            continue;
        }

        // Acknowledge the webhook immediately, then process. Meta retries on
        // non-200, so we always answer 200 once the body is in hand.
        write_response(&mut stream, "200 OK", "ok");

        let payload: Value = serde_json::from_str(&req.body).unwrap_or(Value::Null);
        for msg in parse_incoming(&payload) {
            println!("  whatsapp ← [{}] {}", msg.from, msg.text);
            let hist = histories
                .entry(msg.from.clone())
                .or_insert_with(|| vec![ChatMessage::system(agent::system_prompt())]);
            hist.push(ChatMessage::user(&msg.text));

            // Full agent loop: persona + Python integrations + remember/schedule.
            // Scheduled reports default back to this WhatsApp recipient.
            let origin = format!("whatsapp:{}", msg.from);
            let reply = rt.block_on(agent::run_turn(&model, hist, &origin));

            rt.block_on(send_message(&client, &settings, &msg.from, &reply));
            println!("  whatsapp → [{}] {}", msg.from, reply);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_webhook_messages() {
        let payload = json!({
            "entry": [{
                "changes": [{
                    "value": {
                        "messages": [
                            { "from": "628123", "text": { "body": "hello" } },
                            { "from": "628999", "text": { "body": "hi there" } }
                        ]
                    }
                }]
            }]
        });
        let msgs = parse_incoming(&payload);
        assert_eq!(msgs.len(), 2);
        assert_eq!(
            msgs[0],
            Incoming {
                from: "628123".into(),
                text: "hello".into()
            }
        );
    }

    #[test]
    fn ignores_status_only_payloads() {
        let payload = json!({
            "entry": [{ "changes": [{ "value": { "statuses": [{ "status": "read" }] } }] }]
        });
        assert!(parse_incoming(&payload).is_empty());
    }

    #[test]
    fn verify_handshake_checks_token() {
        let mut q = HashMap::new();
        q.insert("hub.mode".into(), "subscribe".into());
        q.insert("hub.verify_token".into(), "secret".into());
        q.insert("hub.challenge".into(), "12345".into());
        assert_eq!(verify_challenge(&q, "secret"), Some("12345".into()));
        assert_eq!(verify_challenge(&q, "wrong"), None);
    }

    #[test]
    fn target_splits_path_and_query() {
        let (path, q) = split_target("/webhook?hub.mode=subscribe&hub.challenge=99");
        assert_eq!(path, "/webhook");
        assert_eq!(q.get("hub.mode").unwrap(), "subscribe");
        assert_eq!(q.get("hub.challenge").unwrap(), "99");
    }

    #[test]
    fn url_decode_handles_percent_and_plus() {
        assert_eq!(url_decode("a%20b+c"), "a b c");
    }
}
