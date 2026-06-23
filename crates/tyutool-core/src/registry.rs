use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashResult, JobSummary};
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;
use crate::plugins::{
    Bk7231nPlugin, Esp32Plugin, Esp32c3Plugin, Esp32c6Plugin, Esp32s3Plugin, Ln882hPlugin,
    T1Plugin, T2Plugin, T3Plugin, T5AIPlugin,
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
        plugins.insert("ESP32S3".to_string(), Arc::new(Esp32s3Plugin));
        log::debug!("Registered flash plugin: ESP32S3");
        plugins.insert("LN882H".to_string(), Arc::new(Ln882hPlugin));
        log::debug!("Registered flash plugin: LN882H");

        Self { plugins }
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

/// Run a job against the default registry (CLI and Tauri use this).
/// Emits [`FlashEvent::JobSummary`] at the start and [`FlashEvent::Done`] at the end.
pub fn run_job<F>(
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

    let result = if matches!(job.mode, FlashMode::Authorize) {
        log::info!("run_job: Authorize mode on port={}", job.port);
        crate::authorize::run_authorize(job, cancel, &progress)
    } else {
        let reg = default_registry();
        let chip = job.normalized_chip_id();
        let plugin = reg.get(&chip)?;
        plugin.run(job, cancel, &progress)
    };

    let elapsed_secs = start.elapsed().as_secs_f64();
    match result {
        Ok(()) => {
            progress(FlashEvent::Done {
                result: FlashResult::Ok { elapsed_secs },
            });
            log::info!("run_job: completed in {:.1}s", elapsed_secs);
            Ok(())
        }
        Err(crate::error::FlashError::Cancelled) => {
            progress(FlashEvent::Done {
                result: FlashResult::Cancelled { elapsed_secs },
            });
            log::info!("run_job: cancelled after {:.1}s", elapsed_secs);
            Err(crate::error::FlashError::Cancelled)
        }
        Err(e) => {
            progress(FlashEvent::Done {
                result: FlashResult::Err {
                    message: e.to_string(),
                    elapsed_secs,
                },
            });
            log::error!("run_job: failed after {:.1}s: {}", elapsed_secs, e);
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
        assert_eq!(ids.len(), 10);
        assert!(ids.contains(&"BK7231N".to_string()));
        assert!(ids.contains(&"T2".to_string()));
        assert!(ids.contains(&"T3".to_string()));
        assert!(ids.contains(&"T5AI".to_string()));
        assert!(ids.contains(&"T1".to_string()));
        assert!(ids.contains(&"ESP32".to_string()));
        assert!(ids.contains(&"ESP32C3".to_string()));
        assert!(ids.contains(&"ESP32C6".to_string()));
        assert!(ids.contains(&"ESP32S3".to_string()));
        assert!(ids.contains(&"LN882H".to_string()));
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
}
