//! B1 slice integration tests: WS server on 127.0.0.1 with Origin allowlist,
//! hello frame pushed on connect, and no port drift when occupied.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

/// Must be present in the compile-time allowlist (cobuilder-web dev default).
const ALLOWED_DEV_ORIGIN: &str = "http://localhost:3000";

async fn start_server() -> SocketAddr {
    let server = tyutool_bridge::bind(0).await.expect("bind ephemeral port");
    let addr = server.local_addr().expect("local addr");
    tokio::spawn(server.run());
    addr
}

async fn connect_with_origin(
    addr: &SocketAddr,
    origin: &str,
) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!("ws://{addr}/")
        .into_client_request()
        .expect("build client request");
    request.headers_mut().insert(
        "Origin",
        HeaderValue::from_str(origin).expect("valid origin header"),
    );
    tokio_tungstenite::connect_async(request)
        .await
        .map(|(ws, _resp)| ws)
}

#[tokio::test]
async fn whitelisted_origin_receives_hello_with_full_fields() {
    let addr = start_server().await;
    let mut ws = connect_with_origin(&addr, ALLOWED_DEV_ORIGIN)
        .await
        .expect("whitelisted origin must complete the WS handshake");

    let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("hello frame must arrive within 5s")
        .expect("stream must not end before hello")
        .expect("ws read must succeed");
    let text = msg.into_text().expect("hello must be a text frame");
    let v: serde_json::Value = serde_json::from_str(&text).expect("hello must be JSON");

    assert_eq!(v["type"], "hello", "frame: {text}");
    assert_eq!(v["protocol_version"], 1, "frame: {text}");
    assert_eq!(v["app_version"], "0.1.0", "frame: {text}");
    #[cfg(target_os = "macos")]
    assert_eq!(v["platform"], "darwin", "frame: {text}");
    #[cfg(target_os = "windows")]
    assert_eq!(v["platform"], "windows", "frame: {text}");
    #[cfg(target_os = "linux")]
    assert_eq!(v["platform"], "linux", "frame: {text}");
    let os_version = v["os_version"]
        .as_str()
        .expect("os_version must be a string");
    assert!(
        os_version.chars().any(|c| c.is_ascii_digit()),
        "os_version must carry a real OS version number, got {os_version:?}"
    );
}

#[tokio::test]
async fn non_whitelisted_origin_is_rejected_at_handshake() {
    let addr = start_server().await;
    let result = connect_with_origin(&addr, "https://evil.example.com").await;
    assert!(
        result.is_err(),
        "forged origin must be rejected during the WS handshake"
    );
}

#[tokio::test]
async fn bind_fails_when_port_is_occupied() {
    let occupier = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("occupy an ephemeral port");
    let port = occupier.local_addr().expect("occupier addr").port();

    let err = match tyutool_bridge::bind(port).await {
        Ok(_) => panic!("bind must fail when port {port} is already occupied"),
        Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(
        msg.contains(&port.to_string()),
        "startup error must name the occupied port: {msg}"
    );
}
