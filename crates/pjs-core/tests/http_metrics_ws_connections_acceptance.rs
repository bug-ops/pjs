//! Acceptance test for #741 item 2 (track F2, after D and F1): the
//! `pjs_websocket_connections` gauge follows the number of open WebSocket
//! connections. Own binary with one test: the gauge is process-global.

#![cfg_attr(
    all(
        feature = "http-server",
        feature = "metrics",
        feature = "websocket-server",
        feature = "websocket-client"
    ),
    feature(impl_trait_in_assoc_type)
)]
#![cfg(all(
    feature = "http-server",
    feature = "metrics",
    feature = "websocket-server",
    feature = "websocket-client"
))]

mod common;
mod http_metrics_support;

use futures::SinkExt;
use http_metrics_support::{scrape_until, spawn_ws_server, value_of};
use pjson_rs::infrastructure::http::axum_adapter::create_pjs_router;
use pjson_rs::infrastructure::websocket::AxumWebSocketTransport;
use tokio_tungstenite::{connect_async, tungstenite::Message};

fn connections(body: &str) -> Option<f64> {
    value_of(body, "pjs_websocket_connections", &[])
}

#[tokio::test]
async fn the_websocket_connection_gauge_tracks_open_connections() {
    let metrics = create_pjs_router().with_state(common::create_test_app_state());
    let addr = spawn_ws_server(AxumWebSocketTransport::new()).await;
    let url = format!("ws://{addr}/ws");

    let (mut first, _) = connect_async(&url).await.expect("first connects");
    let (mut second, _) = connect_async(&url).await.expect("second connects");
    let both = scrape_until(&metrics, |body| connections(body) == Some(2.0)).await;
    assert_eq!(connections(&both), Some(2.0), "{both}");

    first.send(Message::Close(None)).await.expect("close first");
    drop(first);
    let one = scrape_until(&metrics, |body| connections(body) == Some(1.0)).await;
    assert_eq!(connections(&one), Some(1.0), "{one}");

    second
        .send(Message::Close(None))
        .await
        .expect("close second");
    drop(second);
    let none = scrape_until(&metrics, |body| connections(body) == Some(0.0)).await;
    assert_eq!(connections(&none), Some(0.0), "{none}");
}
