//! Integration tests for the dashboard (Task 21.3): auth enforcement and a
//! dashboard-submitted message round-trip through the Channel trait.

use cyrene_core::{Channel, InboundMessage, OutboundMessage, UserId};
use cyrene_dashboard::{DashboardAuth, DashboardChannel};

#[test]
fn auth_enforcement_rejects_unauthorized_requests() {
    let auth = DashboardAuth::new("super-secret");

    // A request without the token is denied (R26.7).
    assert!(!auth.authorize_header(None));
    assert!(!auth.authorize_header(Some("Bearer nope")));

    // A request with the correct bearer token is granted.
    assert!(auth.authorize_header(Some("Bearer super-secret")));
}

#[tokio::test]
async fn dashboard_message_round_trips_through_the_loop() {
    // Simulate the full path: the WS handler submits an inbound message, the
    // loop polls it, processes it, and sends a response that the dashboard
    // drains to stream back to the browser (R26.3).
    let channel = DashboardChannel::new();

    // 1. Browser submits a chat message.
    channel.submit(InboundMessage::new(
        "dashboard",
        UserId::new("alice"),
        "what is the build status?",
    ));

    // 2. The loop polls the channel (as it would any channel).
    let inbound = channel.poll().await.unwrap().expect("a message is pending");
    assert_eq!(inbound.text, "what is the build status?");
    assert_eq!(inbound.origin, cyrene_core::ChannelId::new("dashboard"));

    // 3. The loop produces a response and sends it on the origin channel.
    let reply = OutboundMessage::reply_to(&inbound, "Build is green.");
    channel.send(reply).await.unwrap();

    // 4. The dashboard drains the response to stream to the browser.
    let responses = channel.drain_responses();
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0].text, "Build is green.");
    assert_eq!(responses[0].user_id, UserId::new("alice"));
}

#[tokio::test]
async fn polling_an_empty_dashboard_channel_yields_none() {
    let channel = DashboardChannel::new();
    assert!(channel.poll().await.unwrap().is_none());
}
