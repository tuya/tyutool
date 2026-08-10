//! B6 slice integration tests: runtime stats published for the tray shell —
//! active connection count and discovered (allowlisted) device count on a
//! watch channel, updated as clients come and go and devices change.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use tyutool_bridge::status::StatsSnapshot;
use tyutool_bridge::{
    EnumeratedPort, FlashBackend, FlashJobSpec, JobError, PortEnumerator, PortProbe,
};

const ALLOWED_DEV_ORIGIN: &str = "http://localhost:3000";

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

struct NoopBackend;

impl FlashBackend for NoopBackend {
    fn run_job(
        &self,
        _spec: FlashJobSpec,
        _cancel: Arc<std::sync::atomic::AtomicBool>,
        _progress: &(dyn Fn(serde_json::Value) + Send + Sync),
    ) -> Result<(), JobError> {
        Ok(())
    }

    fn probe_port(&self, _port: &str) -> PortProbe {
        PortProbe {
            available: true,
            reason: None,
            occupied_by: None,
        }
    }
}

fn wch_port(path: &str) -> EnumeratedPort {
    EnumeratedPort {
        path: path.to_string(),
        vid: Some(0x1A86),
        pid: Some(0x55D2),
        vendor: Some("WCH".to_string()),
        busy: false,
        serial_number: None,
        usb_interface: None,
    }
}

fn unknown_vid_port(path: &str) -> EnumeratedPort {
    EnumeratedPort {
        path: path.to_string(),
        vid: Some(0x1234),
        pid: Some(0x5678),
        vendor: None,
        busy: false,
        serial_number: None,
        usb_interface: None,
    }
}

type FakePorts = Arc<Mutex<Vec<EnumeratedPort>>>;

async fn start_observed_server(
    initial: Vec<EnumeratedPort>,
) -> (
    SocketAddr,
    FakePorts,
    tokio::sync::watch::Receiver<StatsSnapshot>,
) {
    let fake: FakePorts = Arc::new(Mutex::new(initial));
    let source = Arc::clone(&fake);
    let enumerator: PortEnumerator =
        Arc::new(move || source.lock().expect("fake ports lock").clone());
    let (stats_tx, stats_rx) = tokio::sync::watch::channel(StatsSnapshot::default());
    let server = tyutool_bridge::bind(0).await.expect("bind ephemeral port");
    let addr = server.local_addr().expect("local addr");
    tokio::spawn(server.run_observed(
        enumerator,
        Duration::from_millis(20),
        Arc::new(NoopBackend),
        stats_tx,
    ));
    (addr, fake, stats_rx)
}

async fn connect(addr: &SocketAddr) -> Ws {
    let mut request = format!("ws://{addr}/")
        .into_client_request()
        .expect("build client request");
    request.headers_mut().insert(
        "Origin",
        HeaderValue::from_str(ALLOWED_DEV_ORIGIN).expect("valid origin header"),
    );
    let (mut ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .expect("whitelisted origin must connect");
    // Drain hello + initial ports so the connection is fully established.
    for _ in 0..2 {
        let _ = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("startup frame within 2s");
    }
    ws
}

async fn wait_stats(
    rx: &mut tokio::sync::watch::Receiver<StatsSnapshot>,
    what: &str,
    predicate: impl FnMut(&StatsSnapshot) -> bool,
) -> StatsSnapshot {
    let value = tokio::time::timeout(Duration::from_secs(2), rx.wait_for(predicate))
        .await
        .unwrap_or_else(|_| panic!("{what}: stats must update within 2s"))
        .unwrap_or_else(|e| panic!("{what}: stats channel must stay open: {e}"));
    *value
}

#[tokio::test]
async fn connection_count_tracks_clients_joining_and_leaving() {
    let (addr, _fake, mut rx) = start_observed_server(vec![]).await;

    let ws_a = connect(&addr).await;
    let _ws_b = connect(&addr).await;
    wait_stats(&mut rx, "two clients", |s| s.connections == 2).await;

    drop(ws_a);
    wait_stats(&mut rx, "one client left", |s| s.connections == 1).await;
}

#[tokio::test]
async fn device_count_counts_allowlisted_ports_only_and_follows_hotplug() {
    // One allowlisted + one unknown-VID port: only the allowlisted one counts.
    let (_addr, fake, mut rx) = start_observed_server(vec![
        wch_port("/dev/tty.devA"),
        unknown_vid_port("/dev/tty.other"),
    ])
    .await;

    let snapshot = wait_stats(&mut rx, "initial devices", |s| s.devices == 1).await;
    assert_eq!(snapshot.connections, 0, "{snapshot:?}");

    // Hotplug a second allowlisted device.
    {
        let mut guard = fake.lock().expect("fake ports lock");
        guard.push(wch_port("/dev/tty.devB"));
    }
    wait_stats(&mut rx, "hotplug", |s| s.devices == 2).await;

    // Unplug everything.
    {
        let mut guard = fake.lock().expect("fake ports lock");
        guard.clear();
    }
    wait_stats(&mut rx, "unplug", |s| s.devices == 0).await;
}
