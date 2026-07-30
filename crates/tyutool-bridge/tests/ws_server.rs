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

/// Origins a real cobuilder-web deployment is served from, transcribed from
/// cobuilder-web `config/index.cjs` (`base` / `daily` / `pre` / `prod` region
/// maps). Deduplicated: `base` and `daily` share `dev-claw-wb.wgine.com`, and
/// `pre` maps both AZ and SG to `developer-us.wgine.com`.
const DEPLOYED_ORIGINS: &[&str] = &[
    // daily (internal test environment)
    "https://dev-claw-wb.wgine.com",
    // pre
    "https://developer.wgine.com",
    "https://developer-us.wgine.com",
    "https://developer-eu.wgine.com",
    "https://developer-in.wgine.com",
    "https://developer-ue.wgine.com",
    "https://developer-we.wgine.com",
    // prod
    "https://platform.tuya.com",
    "https://us.platform.tuya.com",
    "https://eu.platform.tuya.com",
    "https://ind.platform.tuya.com",
    "https://ue.platform.tuya.com",
    "https://we.platform.tuya.com",
    "https://sg.platform.tuya.com",
];

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
async fn deployed_cobuilder_origins_complete_the_handshake() {
    let addr = start_server().await;
    let mut refused = Vec::new();
    for origin in DEPLOYED_ORIGINS {
        if connect_with_origin(&addr, origin).await.is_err() {
            refused.push(*origin);
        }
    }
    assert!(
        refused.is_empty(),
        "every deployed cobuilder-web origin must complete the WS handshake, refused: {refused:?}"
    );
}

#[tokio::test]
async fn origins_that_only_look_like_deployed_ones_are_still_rejected() {
    let addr = start_server().await;
    // Exact string matching only: no wildcard, no suffix/prefix matching, no
    // scheme relaxation. Each of these would slip through a looser rule.
    let lookalikes = [
        "https://evil.example.com",
        "https://developer.wgine.com.evil.com",
        "https://evil-developer.wgine.com",
        "https://developer.wgine.com.attacker.io",
        "https://attacker.developer.wgine.com",
        "http://developer.wgine.com",
        "https://developer.wgine.com/",
        "https://platform.tuya.com.evil.com",
        "https://evil.platform.tuya.com",
    ];
    let mut accepted = Vec::new();
    for origin in lookalikes {
        if connect_with_origin(&addr, origin).await.is_ok() {
            accepted.push(origin);
        }
    }
    assert!(
        accepted.is_empty(),
        "origins outside the allowlist must be refused at the handshake, accepted: {accepted:?}"
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
