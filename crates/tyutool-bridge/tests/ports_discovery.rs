//! B2 slice integration tests: device auto-discovery — a full `ports` frame
//! right after hello, diff-driven pushes to every connected client, VID
//! allowlist marking, stable `first_seen_ms`, and `busy` passthrough.
//!
//! The enumeration source is injected (fake, mutex-backed) so discovery logic
//! is exercised without real serial hardware.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tyutool_bridge::{EnumeratedPort, PortEnumerator};

/// Must be present in the compile-time allowlist (cobuilder-web dev default).
const ALLOWED_DEV_ORIGIN: &str = "http://localhost:3000";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Shared fake enumeration source the test mutates to simulate hotplug.
type FakePorts = Arc<Mutex<Vec<EnumeratedPort>>>;

async fn start_server_with_fake_ports(initial: Vec<EnumeratedPort>) -> (SocketAddr, FakePorts) {
    let fake: FakePorts = Arc::new(Mutex::new(initial));
    let source = Arc::clone(&fake);
    let enumerator: PortEnumerator =
        Arc::new(move || source.lock().expect("fake ports lock").clone());
    let server = tyutool_bridge::bind(0).await.expect("bind ephemeral port");
    let addr = server.local_addr().expect("local addr");
    tokio::spawn(server.run_with_enumerator(enumerator, Duration::from_millis(20)));
    (addr, fake)
}

async fn connect(addr: &SocketAddr) -> Ws {
    let mut request = format!("ws://{addr}/")
        .into_client_request()
        .expect("build client request");
    request.headers_mut().insert(
        "Origin",
        HeaderValue::from_str(ALLOWED_DEV_ORIGIN).expect("valid origin header"),
    );
    let (ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("whitelisted origin must connect");
    ws
}

async fn next_json(ws: &mut Ws, what: &str) -> serde_json::Value {
    let polled = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .unwrap_or_else(|_| panic!("{what}: frame must arrive within 2s"));
    // `match` instead of `unwrap_or_else` chaining: a closure whose inferred
    // return type is the full Result<Message, tungstenite::Error> would trip
    // clippy::result_large_err.
    let msg = match polled {
        Some(Ok(msg)) => msg,
        Some(Err(e)) => panic!("{what}: ws read must succeed: {e}"),
        None => panic!("{what}: stream must not end"),
    };
    let text = msg
        .into_text()
        .unwrap_or_else(|e| panic!("{what}: must be a text frame: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{what}: must be JSON ({e}): {text}"))
}

/// Wait for the next `ports` frame (skipping nothing: after hello, the bridge
/// only emits ports frames in B2).
async fn next_ports_frame(ws: &mut Ws, what: &str) -> serde_json::Value {
    let v = next_json(ws, what).await;
    assert_eq!(v["type"], "ports", "{what}: expected ports frame, got {v}");
    v
}

fn wch_port(path: &str, busy: bool) -> EnumeratedPort {
    EnumeratedPort {
        path: path.to_string(),
        vid: Some(0x1A86),
        pid: Some(0x55D2),
        vendor: Some("WCH".to_string()),
        busy,
    }
}

#[tokio::test]
async fn connection_receives_full_ports_frame_right_after_hello() {
    let (addr, _fake) = start_server_with_fake_ports(vec![]).await;
    let mut ws = connect(&addr).await;

    let first = next_json(&mut ws, "first frame").await;
    assert_eq!(first["type"], "hello", "first frame must be hello: {first}");

    let ports = next_ports_frame(&mut ws, "initial ports").await;
    let list = ports["ports"].as_array().expect("ports must be an array");
    assert!(list.is_empty(), "no devices enumerated yet: {ports}");
}

#[tokio::test]
async fn device_change_pushes_updated_list_to_all_clients() {
    let (addr, fake) = start_server_with_fake_ports(vec![]).await;

    // Two concurrent clients: both must receive the hotplug push.
    let mut ws_a = connect(&addr).await;
    let mut ws_b = connect(&addr).await;
    for (ws, name) in [(&mut ws_a, "client A"), (&mut ws_b, "client B")] {
        next_json(ws, &format!("{name} hello")).await;
        next_ports_frame(ws, &format!("{name} initial ports")).await;
    }

    // Simulate plugging in one allowlisted and one unknown-VID device.
    {
        let mut guard = fake.lock().expect("fake ports lock");
        *guard = vec![
            wch_port("/dev/tty.wchusbserial56D70347441", false),
            EnumeratedPort {
                path: "/dev/tty.usbmodem_unknown1".to_string(),
                vid: Some(0x1234),
                pid: Some(0x5678),
                vendor: None,
                busy: false,
            },
        ];
    }

    for (ws, name) in [(&mut ws_a, "client A"), (&mut ws_b, "client B")] {
        let frame = next_ports_frame(ws, &format!("{name} hotplug push")).await;
        let list = frame["ports"].as_array().expect("ports array");
        assert_eq!(list.len(), 2, "{name}: full list expected: {frame}");

        let wch = list
            .iter()
            .find(|p| p["port"] == "/dev/tty.wchusbserial56D70347441")
            .unwrap_or_else(|| panic!("{name}: WCH port missing: {frame}"));
        assert_eq!(wch["vid"], "1A86", "vid must be uppercase hex: {wch}");
        assert_eq!(wch["pid"], "55D2", "pid must be uppercase hex: {wch}");
        assert_eq!(wch["vendor"], "WCH", "{wch}");
        assert_eq!(wch["whitelisted"], true, "WCH VID is allowlisted: {wch}");
        assert_eq!(wch["busy"], false, "{wch}");

        let unknown = list
            .iter()
            .find(|p| p["port"] == "/dev/tty.usbmodem_unknown1")
            .unwrap_or_else(|| panic!("{name}: unknown port missing: {frame}"));
        assert_eq!(
            unknown["whitelisted"], false,
            "unknown VID must still be pushed, gray-listed: {unknown}"
        );
    }
}

#[tokio::test]
async fn replugged_device_gets_a_fresh_first_seen_ms() {
    let (addr, fake) =
        start_server_with_fake_ports(vec![wch_port("/dev/tty.replugA", false)]).await;
    let mut ws = connect(&addr).await;
    next_json(&mut ws, "hello").await;

    let initial = next_ports_frame(&mut ws, "initial ports").await;
    let original_seen = initial["ports"][0]["first_seen_ms"]
        .as_u64()
        .expect("first_seen_ms must be a u64 timestamp");

    // Unplug: the device disappears, an empty full list is pushed.
    {
        let mut guard = fake.lock().expect("fake ports lock");
        guard.clear();
    }
    let unplugged = next_ports_frame(&mut ws, "unplug push").await;
    assert!(
        unplugged["ports"]
            .as_array()
            .expect("ports array")
            .is_empty(),
        "unplug must push the (empty) full list: {unplugged}"
    );

    // Give the clock a strictly later millisecond before replugging.
    tokio::time::sleep(Duration::from_millis(30)).await;

    // Replug the same path: it must be treated as a new device (fresh stamp).
    {
        let mut guard = fake.lock().expect("fake ports lock");
        *guard = vec![wch_port("/dev/tty.replugA", false)];
    }
    let replugged = next_ports_frame(&mut ws, "replug push").await;
    let fresh_seen = replugged["ports"][0]["first_seen_ms"]
        .as_u64()
        .expect("replugged first_seen_ms");
    assert!(
        fresh_seen > original_seen,
        "replug must restamp first_seen_ms (PRD: 重插按新设备): {fresh_seen} <= {original_seen}"
    );
}

#[tokio::test]
async fn first_seen_ms_is_stable_across_pushes_and_busy_is_passed_through() {
    let (addr, fake) =
        start_server_with_fake_ports(vec![wch_port("/dev/tty.stableA", false)]).await;
    let mut ws = connect(&addr).await;
    next_json(&mut ws, "hello").await;

    let initial = next_ports_frame(&mut ws, "initial ports").await;
    let list = initial["ports"].as_array().expect("ports array");
    assert_eq!(list.len(), 1, "{initial}");
    let first_seen = list[0]["first_seen_ms"]
        .as_u64()
        .expect("first_seen_ms must be a u64 timestamp");
    assert!(first_seen > 0, "first_seen_ms must be a real timestamp");

    // Second device appears (busy), first device unchanged: its first_seen_ms
    // must not move even though the full list is re-pushed.
    {
        let mut guard = fake.lock().expect("fake ports lock");
        *guard = vec![
            wch_port("/dev/tty.stableA", false),
            wch_port("/dev/tty.newcomerB", true),
        ];
    }

    let updated = next_ports_frame(&mut ws, "hotplug push").await;
    let list = updated["ports"].as_array().expect("ports array");
    assert_eq!(list.len(), 2, "{updated}");

    let stable = list
        .iter()
        .find(|p| p["port"] == "/dev/tty.stableA")
        .unwrap_or_else(|| panic!("stableA missing: {updated}"));
    assert_eq!(
        stable["first_seen_ms"].as_u64(),
        Some(first_seen),
        "first_seen_ms must stay stable across repeated enumeration: {stable}"
    );

    let newcomer = list
        .iter()
        .find(|p| p["port"] == "/dev/tty.newcomerB")
        .unwrap_or_else(|| panic!("newcomerB missing: {updated}"));
    assert_eq!(
        newcomer["busy"], true,
        "busy flag must pass through: {newcomer}"
    );
    let newcomer_seen = newcomer["first_seen_ms"]
        .as_u64()
        .expect("newcomer first_seen_ms");
    assert!(
        newcomer_seen >= first_seen,
        "newcomer must not predate an earlier device: {newcomer_seen} < {first_seen}"
    );
}
