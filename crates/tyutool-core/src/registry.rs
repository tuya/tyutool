use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashResult, JobSummary};
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;
use crate::plugins::{
    Bk7231nPlugin, Esp32Plugin, Esp32c3Plugin, Esp32c6Plugin, Esp32p4Plugin, Esp32s3Plugin,
    Gd32vw553Plugin, Ln882hPlugin, T1Plugin, T2Plugin, T3Plugin, T5AIPlugin,
};

/// Canonicalize a user-supplied chip id: trim, upper-case, and rewrite legacy
/// names to their current registry key. Used at every chip-id boundary
/// (registry lookup, [`FlashJob::normalized_chip_id`], serial reset routing)
/// so callers passing the legacy `T5` keep working after the T5→T5AI rename.
pub fn normalize_chip_id(raw: &str) -> String {
    let key = raw.trim().to_ascii_uppercase();
    match key.as_str() {
        "T5" => "T5AI".to_string(),
        _ => key,
    }
}

/// Registry key of the fake device supplied by the `mock-chip` feature.
///
/// A job carrying this chip id runs [`crate::plugins::mock::MockPlugin::simulated`]
/// instead of touching a serial port. Only present when the feature is on, which
/// no shipped build may do.
#[cfg(feature = "mock-chip")]
pub const MOCK_CHIP_ID: &str = "MOCK";

/// Global registry of chip plugins (Python `FlashInterface.SocList` equivalent).
pub struct FlashPluginRegistry {
    plugins: HashMap<String, Arc<dyn FlashPlugin>>,
}

impl FlashPluginRegistry {
    pub fn new() -> Self {
        let mut plugins: HashMap<String, Arc<dyn FlashPlugin>> = HashMap::new();

        plugins.insert("BK7231N".to_string(), Arc::new(Bk7231nPlugin));
        log::debug!("Registered flash plugin: BK7231N");
        plugins.insert("T2".to_string(), Arc::new(T2Plugin));
        log::debug!("Registered flash plugin: T2");
        plugins.insert("T3".to_string(), Arc::new(T3Plugin));
        log::debug!("Registered flash plugin: T3");
        plugins.insert("T5AI".to_string(), Arc::new(T5AIPlugin));
        log::debug!("Registered flash plugin: T5AI");
        plugins.insert("T1".to_string(), Arc::new(T1Plugin));
        log::debug!("Registered flash plugin: T1");
        plugins.insert("ESP32".to_string(), Arc::new(Esp32Plugin));
        log::debug!("Registered flash plugin: ESP32");
        plugins.insert("ESP32C3".to_string(), Arc::new(Esp32c3Plugin));
        log::debug!("Registered flash plugin: ESP32C3");
        plugins.insert("ESP32C6".to_string(), Arc::new(Esp32c6Plugin));
        log::debug!("Registered flash plugin: ESP32C6");
        plugins.insert("ESP32P4".to_string(), Arc::new(Esp32p4Plugin));
        log::debug!("Registered flash plugin: ESP32P4");
        plugins.insert("ESP32S3".to_string(), Arc::new(Esp32s3Plugin));
        log::debug!("Registered flash plugin: ESP32S3");
        plugins.insert("LN882H".to_string(), Arc::new(Ln882hPlugin));
        log::debug!("Registered flash plugin: LN882H");
        plugins.insert("GD32VW553".to_string(), Arc::new(Gd32vw553Plugin));
        log::debug!("Registered flash plugin: GD32VW553");

        // A fake device in the default registry, so `run_job` — and therefore
        // every frontend, unchanged — can drive a job that behaves like
        // hardware without any. See the `mock-chip` feature in Cargo.toml for
        // the two guards that keep it out of a shipped artifact.
        #[cfg(feature = "mock-chip")]
        {
            plugins.insert(
                MOCK_CHIP_ID.to_string(),
                Arc::new(crate::plugins::mock::MockPlugin::simulated(MOCK_CHIP_ID)),
            );
            log::debug!("Registered flash plugin: {}", MOCK_CHIP_ID);
        }

        Self { plugins }
    }

    /// Register a plugin under its own [`FlashPlugin::id`], replacing whatever
    /// was held under that id before.
    ///
    /// The key goes through [`normalize_chip_id`] so registration and
    /// [`get`](Self::get) agree on one spelling. Replacement is deliberate: a
    /// test can swap a real chip id (`T5AI`) for a scripted stand-in and so
    /// exercise the path a frontend actually takes, instead of a made-up id no
    /// caller would ever send.
    pub fn register(&mut self, plugin: Arc<dyn FlashPlugin>) {
        let key = normalize_chip_id(plugin.id());
        log::debug!("Registered flash plugin: {}", key);
        self.plugins.insert(key, plugin);
    }

    pub fn get(&self, chip_id: &str) -> Result<&Arc<dyn FlashPlugin>, FlashError> {
        let key = normalize_chip_id(chip_id);
        self.plugins.get(&key).ok_or(FlashError::UnknownChip(key))
    }

    pub fn list_chip_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.plugins.keys().cloned().collect();
        v.sort();
        v
    }
}

impl Default for FlashPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

static GLOBAL_REGISTRY: OnceLock<FlashPluginRegistry> = OnceLock::new();

pub fn default_registry() -> &'static FlashPluginRegistry {
    GLOBAL_REGISTRY.get_or_init(FlashPluginRegistry::new)
}

/// Run a job against the default registry (CLI, serve and Tauri all use this).
/// Emits [`FlashEvent::JobSummary`] at the start and [`FlashEvent::Done`] at the end.
pub fn run_job<F>(
    job: &FlashJob,
    cancel: &std::sync::atomic::AtomicBool,
    progress: F,
) -> Result<(), FlashError>
where
    F: Fn(FlashEvent),
{
    run_job_with(default_registry(), job, cancel, progress)
}

/// [`run_job`] against an explicit registry.
///
/// The orchestration lives here and [`run_job`] is a one-line wrapper, so there
/// is exactly one implementation of the event contract. Tests build a registry
/// holding a scripted plugin (see the `mock-chip` feature) and drive this
/// directly; production never passes anything but [`default_registry`].
pub fn run_job_with<F>(
    registry: &FlashPluginRegistry,
    job: &FlashJob,
    cancel: &std::sync::atomic::AtomicBool,
    progress: F,
) -> Result<(), FlashError>
where
    F: Fn(FlashEvent),
{
    let start = std::time::Instant::now();
    progress(FlashEvent::JobSummary(JobSummary::from_job(job)));

    log::info!(
        "run_job: chip={}, port={}, mode={:?}",
        job.normalized_chip_id(),
        job.port,
        job.mode
    );

    match job.to_cli_command() {
        Some(cmd) => log::info!("run_job: equivalent CLI command: {}", cmd),
        None => log::info!(
            "run_job: no equivalent single CLI command (multi-segment job or a required field is unset)"
        ),
    }

    let result = if matches!(job.mode, FlashMode::Authorize) {
        log::info!("run_job: Authorize mode on port={}", job.port);
        crate::authorize::run_authorize(job, cancel, &progress)
    } else {
        let chip = job.normalized_chip_id();
        let plugin = registry.get(&chip)?;
        plugin.run(job, cancel, &progress)
    };

    let elapsed_secs = start.elapsed().as_secs_f64();
    match result {
        Ok(()) => {
            progress(FlashEvent::Done {
                result: FlashResult::Ok { elapsed_secs },
            });
            log::info!(
                "run_job: port={} completed in {:.1}s",
                job.port,
                elapsed_secs
            );
            Ok(())
        }
        Err(crate::error::FlashError::Cancelled) => {
            progress(FlashEvent::Done {
                result: FlashResult::Cancelled { elapsed_secs },
            });
            log::info!(
                "run_job: port={} cancelled after {:.1}s",
                job.port,
                elapsed_secs
            );
            Err(crate::error::FlashError::Cancelled)
        }
        Err(e) => {
            progress(FlashEvent::Done {
                result: FlashResult::Err {
                    message: e.to_string(),
                    elapsed_secs,
                },
            });
            log::error!(
                "run_job: port={} failed after {:.1}s: {}",
                job.port,
                elapsed_secs,
                e
            );
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::FlashMode;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn normalize_chip_id_maps_legacy_t5_to_t5ai() {
        assert_eq!(normalize_chip_id("T5"), "T5AI");
        assert_eq!(normalize_chip_id("t5"), "T5AI");
        assert_eq!(normalize_chip_id("  T5  "), "T5AI");
    }

    #[test]
    fn normalize_chip_id_leaves_other_ids_alone() {
        assert_eq!(normalize_chip_id("T5AI"), "T5AI");
        assert_eq!(normalize_chip_id("esp32"), "ESP32");
        assert_eq!(normalize_chip_id("BK7231N"), "BK7231N");
    }

    #[test]
    fn registry_get_accepts_legacy_t5_id() {
        let r = FlashPluginRegistry::new();
        let via_legacy = r.get("T5").expect("legacy T5 should resolve");
        let via_canonical = r.get("T5AI").expect("canonical T5AI should resolve");
        assert!(
            Arc::ptr_eq(via_legacy, via_canonical),
            "T5 must alias to the same plugin as T5AI",
        );
        assert!(r.get("t5").is_ok(), "lowercase t5 alias should also work");
    }

    #[test]
    fn registry_has_all_chips() {
        let r = FlashPluginRegistry::new();
        assert!(r.get("bk7231n").is_ok());
        assert!(r.get("BK7231N").is_ok());
        assert!(r.get("t2").is_ok());
        assert!(r.get("T2").is_ok());
        assert!(r.get("t3").is_ok());
        assert!(r.get("T3").is_ok());
        assert!(r.get("t5ai").is_ok());
        assert!(r.get("T5AI").is_ok());
        assert!(r.get("t1").is_ok());
        assert!(r.get("T1").is_ok());
        assert!(r.get("esp32").is_ok());
        assert!(r.get("ESP32").is_ok());
        assert!(r.get("esp32c3").is_ok());
        assert!(r.get("ESP32C3").is_ok());
        assert!(r.get("esp32c6").is_ok());
        assert!(r.get("ESP32C6").is_ok());
        assert!(r.get("esp32p4").is_ok());
        assert!(r.get("ESP32P4").is_ok());
        assert!(r.get("esp32s3").is_ok());
        assert!(r.get("ESP32S3").is_ok());
        assert!(r.get("ln882h").is_ok());
        assert!(r.get("LN882H").is_ok());
        assert!(r.get("unknown").is_err());
    }

    #[test]
    fn list_chip_ids_only_real_plugins() {
        let r = FlashPluginRegistry::new();
        let ids = r.list_chip_ids();
        // The 12 real chips, plus the fake device when `mock-chip` is on. Any
        // other entry means something got registered that should not have been.
        let expected = if cfg!(feature = "mock-chip") { 13 } else { 12 };
        assert_eq!(ids.len(), expected, "unexpected registry contents: {ids:?}");
        assert!(ids.contains(&"BK7231N".to_string()));
        assert!(ids.contains(&"T2".to_string()));
        assert!(ids.contains(&"T3".to_string()));
        assert!(ids.contains(&"T5AI".to_string()));
        assert!(ids.contains(&"T1".to_string()));
        assert!(ids.contains(&"ESP32".to_string()));
        assert!(ids.contains(&"ESP32C3".to_string()));
        assert!(ids.contains(&"ESP32C6".to_string()));
        assert!(ids.contains(&"ESP32P4".to_string()));
        assert!(ids.contains(&"ESP32S3".to_string()));
        assert!(ids.contains(&"LN882H".to_string()));
        assert!(ids.contains(&"GD32VW553".to_string()));
    }

    #[test]
    fn unknown_chip_returns_error() {
        let _r = FlashPluginRegistry::new();
        let job = FlashJob {
            mode: FlashMode::Flash,
            chip_id: "NONEXISTENT".into(),
            port: "/dev/null".into(),
            baud_rate: 115200,
            segments: None,
            flash_start_hex: None,
            flash_end_hex: None,
            erase_start_hex: None,
            erase_end_hex: None,
            read_start_hex: None,
            read_end_hex: None,
            read_file_path: None,
            firmware_path: None,
            authorize_uuid: None,
            authorize_key: None,
            authorize_storage: None,
            confirm_overwrite: None,
        };
        let cancel = AtomicBool::new(false);
        let res = run_job(&job, &cancel, |_| {});
        assert!(res.is_err());
    }

    #[test]
    fn authorize_mode_dispatches_before_chip_lookup() {
        // With mode=Authorize and a bad port, run_job should attempt the auth
        // flow (not the chip registry), fail with a serial-open error, and emit
        // Done{result: Err{..}} — never touching the chip registry.
        use crate::flash_event::{FlashEvent, FlashResult};

        let job = FlashJob {
            mode: FlashMode::Authorize,
            chip_id: "NONEXISTENT".into(),
            port: "/dev/this_port_does_not_exist_tyutool_test".into(),
            baud_rate: 115200,
            segments: None,
            flash_start_hex: None,
            flash_end_hex: None,
            erase_start_hex: None,
            erase_end_hex: None,
            read_start_hex: None,
            read_end_hex: None,
            read_file_path: None,
            firmware_path: None,
            authorize_uuid: None,
            authorize_key: None,
            authorize_storage: None,
            confirm_overwrite: None,
        };
        let cancel = AtomicBool::new(false);
        let saw_done = AtomicBool::new(false);
        let res = run_job(&job, &cancel, |p| {
            if let FlashEvent::Done {
                result: FlashResult::Err { .. },
            } = p
            {
                saw_done.store(true, Ordering::SeqCst);
            }
        });
        assert!(res.is_err());
        assert!(saw_done.load(Ordering::SeqCst), "expected Done progress");
        // Error must NOT be UnknownChip — confirms chip lookup was bypassed.
        if let Err(FlashError::UnknownChip(_)) = res {
            panic!("authorize mode must not reach chip registry");
        }
    }

    /// [`run_job_with`]'s orchestration, driven over a scripted plugin.
    ///
    /// These assert the contract every frontend depends on — `JobSummary`
    /// first, `Done` last, a plugin error surfaced as `Done{Err}`, a cancel
    /// surfaced as `Done{Cancelled}` — and **nothing about any chip protocol**:
    /// the plugin here never opens a port. See `plugins::mock` for the full
    /// note on what a mock plugin does and does not buy.
    #[cfg(feature = "mock-chip")]
    mod orchestration {
        use super::*;
        use crate::flash_event::FlashResult;
        use crate::plugins::mock::MockPlugin;
        use std::sync::Mutex;

        /// A full registry with one scripted plugin layered on top. Registering
        /// under a **real** chip id keeps the tests on the path a frontend
        /// takes, rather than an invented id no caller would ever send.
        fn registry_with(plugin: MockPlugin) -> FlashPluginRegistry {
            let mut reg = FlashPluginRegistry::new();
            reg.register(Arc::new(plugin));
            reg
        }

        fn job(chip_id: &str) -> FlashJob {
            FlashJob::new(FlashMode::Flash, chip_id, "/dev/mock", 115_200)
        }

        fn kind(event: &FlashEvent) -> &'static str {
            match event {
                FlashEvent::JobSummary(_) => "job_summary",
                FlashEvent::Phase { .. } => "phase",
                FlashEvent::Percent { .. } => "percent",
                FlashEvent::Milestone { .. } => "milestone",
                FlashEvent::Warning { .. } => "warning",
                FlashEvent::Done { .. } => "done",
            }
        }

        #[test]
        fn success_brackets_the_plugin_events_with_job_summary_and_done() {
            let reg = registry_with(MockPlugin::ok("T5AI"));
            let cancel = AtomicBool::new(false);
            let seen: Mutex<Vec<FlashEvent>> = Mutex::new(Vec::new());

            let res = run_job_with(&reg, &job("T5AI"), &cancel, |e| {
                seen.lock().unwrap().push(e);
            });

            assert!(res.is_ok(), "mock plugin succeeds: {res:?}");
            let seen = seen.lock().unwrap();
            assert_eq!(
                seen.iter().map(kind).collect::<Vec<_>>(),
                ["job_summary", "phase", "milestone", "percent", "done"],
                "JobSummary must open the run and Done must close it",
            );
            assert!(matches!(
                seen.last(),
                Some(FlashEvent::Done {
                    result: FlashResult::Ok { .. }
                })
            ));
        }

        #[test]
        fn plugin_error_is_returned_and_also_reported_as_done_err() {
            let reg = registry_with(MockPlugin::failing("T5AI", "flash id mismatch"));
            let cancel = AtomicBool::new(false);
            let seen: Mutex<Vec<FlashEvent>> = Mutex::new(Vec::new());

            let res = run_job_with(&reg, &job("T5AI"), &cancel, |e| {
                seen.lock().unwrap().push(e);
            });

            assert!(matches!(res, Err(FlashError::Plugin(ref m)) if m == "flash id mismatch"));
            // A frontend that only listens to events must still learn it failed,
            // and must see the plugin's own wording — not a generic message.
            let seen = seen.lock().unwrap();
            match seen.last() {
                Some(FlashEvent::Done {
                    result: FlashResult::Err { message, .. },
                }) => assert_eq!(message, "flash id mismatch"),
                other => panic!("expected Done{{Err}}, got {other:?}"),
            }
        }

        #[test]
        fn cancelling_mid_run_is_reported_as_done_cancelled() {
            let reg = registry_with(MockPlugin::blocking_until_cancelled("T5AI"));
            let cancel = AtomicBool::new(false);
            let seen: Mutex<Vec<FlashEvent>> = Mutex::new(Vec::new());

            let res = run_job_with(&reg, &job("T5AI"), &cancel, |e| {
                // The plugin emits this synchronously *before* it starts polling
                // the flag, so raising it here cancels the run deterministically
                // — no sleeps and no cross-thread race.
                if matches!(e, FlashEvent::Phase { .. }) {
                    cancel.store(true, Ordering::SeqCst);
                }
                seen.lock().unwrap().push(e);
            });

            assert!(matches!(res, Err(FlashError::Cancelled)), "got {res:?}");
            assert!(
                matches!(
                    seen.lock().unwrap().last(),
                    Some(FlashEvent::Done {
                        result: FlashResult::Cancelled { .. }
                    })
                ),
                "a cancelled run must close with Done{{Cancelled}}, not Done{{Err}}",
            );
        }

        #[test]
        fn register_replaces_the_plugin_already_held_under_that_id() {
            let reg = registry_with(MockPlugin::failing("T5AI", "stand-in ran"));
            let cancel = AtomicBool::new(false);

            let res = run_job_with(&reg, &job("T5AI"), &cancel, |_| {});

            // The real T5AI plugin would have failed trying to open /dev/mock;
            // this exact message proves the stand-in took its place.
            assert!(matches!(res, Err(FlashError::Plugin(ref m)) if m == "stand-in ran"));
        }

        #[test]
        fn job_reaches_the_plugin_intact_via_the_legacy_chip_id() {
            let received: Arc<Mutex<Option<(String, String, u32)>>> = Arc::default();
            let sink = Arc::clone(&received);
            let reg = registry_with(MockPlugin::with("T5AI", move |job, _cancel, _progress| {
                *sink.lock().unwrap() =
                    Some((job.chip_id.clone(), job.port.clone(), job.baud_rate));
                Ok(())
            }));
            let cancel = AtomicBool::new(false);

            // `t5` is the legacy spelling; lookup normalizes it to `T5AI`.
            run_job_with(&reg, &job("t5"), &cancel, |_| {}).expect("mock plugin succeeds");

            assert_eq!(
                received.lock().unwrap().clone(),
                // Normalization applies to the *lookup* only — the plugin still
                // sees the job exactly as the caller submitted it.
                Some(("t5".to_string(), "/dev/mock".to_string(), 115_200)),
            );
        }

        #[test]
        fn default_registry_offers_the_mock_chip() {
            let reg = default_registry();
            assert!(reg.get(MOCK_CHIP_ID).is_ok());
            assert!(reg.get("mock").is_ok(), "chip ids are case-insensitive");
        }

        /// The payoff of putting the fake device in the *default* registry: a
        /// frontend needs no new API to drive it. This goes through the plain
        /// [`run_job`] — byte for byte the call `tyutool-cli`, `tyutool-serve`
        /// and `src-tauri` already make — and cancels it mid-flight.
        #[test]
        fn mock_chip_runs_through_plain_run_job_and_honours_cancel() {
            use std::sync::mpsc;

            let cancel = Arc::new(AtomicBool::new(false));
            let (tx, rx) = mpsc::channel::<FlashEvent>();

            let worker_cancel = Arc::clone(&cancel);
            let worker = std::thread::spawn(move || {
                let job = FlashJob::new(FlashMode::Flash, MOCK_CHIP_ID, "/dev/mock", 115_200);
                run_job(&job, &worker_cancel, move |e| {
                    let _ = tx.send(e);
                })
            });

            // Wait until the run is demonstrably underway before cancelling, so
            // this exercises a mid-flight cancel and not one that won a race
            // against the job even starting.
            let saw_progress = rx
                .iter()
                .any(|event| matches!(event, FlashEvent::Percent { .. }));
            assert!(saw_progress, "the mock chip should report progress");
            cancel.store(true, Ordering::SeqCst);

            let res = worker.join().expect("worker thread should not panic");
            assert!(matches!(res, Err(FlashError::Cancelled)), "got {res:?}");

            let tail: Vec<FlashEvent> = rx.into_iter().collect();
            assert!(
                matches!(
                    tail.last(),
                    Some(FlashEvent::Done {
                        result: FlashResult::Cancelled { .. }
                    })
                ),
                "a cancelled run must still close with Done{{Cancelled}}; tail was {tail:?}",
            );
        }
    }
}
