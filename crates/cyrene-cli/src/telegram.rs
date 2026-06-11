//! Minimal Telegram Bot API bridge.
//!
//! Long-polls `getUpdates` and answers every incoming message with the
//! configured model, keeping a short per-chat conversation history. This turns
//! "connect Telegram" into a real, working bot — no external service, no Zapier
//! — using only the bot token from `@BotFather`.
//!
//! Security: the bot replies to anyone who messages it (that's how Telegram
//! bots work), and the token is treated as a secret — it is never printed.

use std::collections::HashMap;
use std::sync::Arc;

use cyrene_core::{ChatMessage, Model, ModelRequest};
use serde_json::Value;

const SYSTEM_PROMPT: &str = "You are Cyrene, the AI agent that always loves you. \
     Be warm, supportive, and concise, and help the user get things done.";

/// Detects a Telegram bot token (`<digits>:<~35 url-safe chars>`) anywhere in
/// `text`. Lets a pasted token in chat trigger a real connection instead of a
/// model reply.
#[must_use]
pub fn detect_token(text: &str) -> Option<String> {
    for word in text.split_whitespace() {
        if let Some((id, rest)) = word.split_once(':') {
            let id_ok = id.len() >= 6 && id.bytes().all(|b| b.is_ascii_digit());
            let rest_ok = rest.len() >= 30
                && rest
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            if id_ok && rest_ok {
                return Some(word.to_owned());
            }
        }
    }
    None
}

/// Verifies the token and returns the bot's `@username` via `getMe`.
async fn get_me(client: &reqwest::Client, token: &str) -> Result<String, String> {
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let json: Value = resp.json().await.map_err(|e| e.to_string())?;
    if json["ok"].as_bool() != Some(true) {
        return Err(json["description"]
            .as_str()
            .unwrap_or("getMe failed")
            .to_owned());
    }
    Ok(json["result"]["username"]
        .as_str()
        .unwrap_or("bot")
        .to_owned())
}

async fn send_message(client: &reqwest::Client, token: &str, chat_id: i64, text: &str) {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let body = serde_json::json!({ "chat_id": chat_id, "text": text });
    if let Err(e) = client.post(&url).json(&body).send().await {
        eprintln!("  telegram: send error: {e}");
    }
}

/// The long-poll loop: fetch updates, answer each text message, repeat. Runs
/// until the process is interrupted.
async fn run_loop(client: reqwest::Client, token: String, model: Arc<dyn Model>) {
    let mut offset: i64 = 0;
    let mut histories: HashMap<i64, Vec<ChatMessage>> = HashMap::new();

    loop {
        let url =
            format!("https://api.telegram.org/bot{token}/getUpdates?timeout=30&offset={offset}");
        let resp = match client
            .get(&url)
            .timeout(std::time::Duration::from_secs(45))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  telegram: poll error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                continue;
            }
        };
        let json: Value = match resp.json().await {
            Ok(j) => j,
            Err(e) => {
                eprintln!("  telegram: decode error: {e}");
                continue;
            }
        };
        let Some(updates) = json["result"].as_array() else {
            continue;
        };

        for upd in updates {
            if let Some(id) = upd["update_id"].as_i64() {
                offset = offset.max(id + 1);
            }
            let msg = &upd["message"];
            let (Some(text), Some(chat_id)) = (msg["text"].as_str(), msg["chat"]["id"].as_i64())
            else {
                continue;
            };
            let from = msg["from"]["first_name"].as_str().unwrap_or("user");
            println!("  telegram ← [{chat_id}] {from}: {text}");

            let hist = histories
                .entry(chat_id)
                .or_insert_with(|| vec![ChatMessage::system(SYSTEM_PROMPT)]);
            hist.push(ChatMessage::user(text));

            let reply = match model.complete(ModelRequest::new(hist.clone())).await {
                Ok(r) => {
                    let reply = r.content.trim().to_owned();
                    hist.push(ChatMessage::assistant(reply.clone()));
                    reply
                }
                Err(e) => {
                    hist.pop();
                    format!("Sorry, I hit an error reaching my model: {e}")
                }
            };

            let reply = if reply.is_empty() {
                "(no response)".to_owned()
            } else {
                reply
            };
            send_message(&client, &token, chat_id, &reply).await;
            println!("  telegram → [{chat_id}] {reply}");
        }
    }
}

/// Starts the Telegram bridge, blocking until the process is interrupted.
/// Verifies the token first and prints the connected bot handle.
pub fn run(rt: &tokio::runtime::Runtime, model: Arc<dyn Model>, token: &str) {
    let client = reqwest::Client::new();
    match rt.block_on(get_me(&client, token)) {
        Ok(username) => {
            println!(
                "✓ Connected to Telegram as @{username}. Listening for messages… (Ctrl-C to stop)\n"
            );
        }
        Err(e) => {
            eprintln!("  ✗ Telegram connection failed: {e}");
            eprintln!("    Double-check the bot token from @BotFather.");
            return;
        }
    }
    rt.block_on(run_loop(client, token.to_owned(), model));
}
