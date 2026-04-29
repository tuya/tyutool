use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::error::FlashError;
use crate::job::{FlashJob, FlashMode};
use crate::plugin::FlashPlugin;
use crate::plugins::{
    Bk7231nPlugin, Esp32Plugin, Esp32c3Plugin, Esp32c6Plugin, Esp32s3Plugin, T1Plugin, T2Plugin,
    T3Plugin, T5Plugin,
};
use crate::progress::FlashProgress;

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
        plugins.insert("T5".to_string(), Arc::new(T5Plugin));
        log::debug!("Registered flash plugin: T5");
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

        Self { plugins }
    }

    pub fn get(&self, chip_id: &str) -> Result<&Arc<dyn FlashPlugin>, FlashError> {
        let key = chip_id.trim().to_ascii_uppercase();
        self.plugins
            .get(&key)
            .ok_or_else(|| FlashError::UnknownChip(key))
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
/// Emits [`FlashProgress::Done`] exactly once at the end (success or failure).
pub fn run_job<F>(
    job: &FlashJob,
    cancel: &std::sync::atomic::AtomicBool,
    progress: F,
) -> Result<(), FlashError>
where
    F: Fn(FlashProgress),
{
    log::info!(
        "run_job: chip={}, port={}, mode={:?}",
        job.normalized_chip_id(),
        job.port,
        job.mode
    );

    // TuyaOpen device authorization is UART text-command based, entirely independent of any
    // chip's BootROM or ESP stub flash protocol — handle before plugin dispatch.
    if matches!(job.mode, FlashMode::Authorize) {
        log::info!(
            "run_job: Authorize mode — running UART auth protocol on port={}",
            job.port
        );
        match crate::authorize::run_authorize(job, cancel, &progress) {
            Ok(()) => {
                progress(FlashProgress::Done {
                    ok: true,
                    message: None,
                });
                log::info!("run_job: authorize completed successfully");
                return Ok(());
            }
            Err(e) => {
                log::error!("run_job: authorize failed: {}", e);
                progress(FlashProgress::Done {
                    ok: false,
                    message: Some(e.to_string()),
                });
                return Err(e);
            }
        }
    }

    let reg = default_registry();
    let chip = job.normalized_chip_id();
    let plugin = reg.get(&chip)?;
    match plugin.run(job, cancel, &progress) {
        Ok(()) => {
            progress(FlashProgress::Done {
                ok: true,
                message: None,
            });
            log::info!("run_job: completed successfully");
            Ok(())
        }
        Err(e) => {
            log::error!("run_job: failed: {}", e);
            progress(FlashProgress::Done {
                ok: false,
                message: Some(e.to_string()),
            });
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
    fn registry_has_all_chips() {
        let r = FlashPluginRegistry::new();
        assert!(r.get("bk7231n").is_ok());
        assert!(r.get("BK7231N").is_ok());
        assert!(r.get("t2").is_ok());
        assert!(r.get("T2").is_ok());
        assert!(r.get("t3").is_ok());
        assert!(r.get("T3").is_ok());
        assert!(r.get("t5").is_ok());
        assert!(r.get("T5").is_ok());
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
        assert!(r.get("unknown").is_err());
    }

    #[test]
    fn list_chip_ids_only_real_plugins() {
        let r = FlashPluginRegistry::new();
        let ids = r.list_chip_ids();
        assert_eq!(ids.len(), 9);
        assert!(ids.contains(&"BK7231N".to_string()));
        assert!(ids.contains(&"T2".to_string()));
        assert!(ids.contains(&"T3".to_string()));
        assert!(ids.contains(&"T5".to_string()));
        assert!(ids.contains(&"T1".to_string()));
        assert!(ids.contains(&"ESP32".to_string()));
        assert!(ids.contains(&"ESP32C3".to_string()));
        assert!(ids.contains(&"ESP32C6".to_string()));
        assert!(ids.contains(&"ESP32S3".to_string()));
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
        // Done{ok:false} — never touching the chip registry.
        use crate::progress::FlashProgress;

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
            if let FlashProgress::Done { ok, .. } = p {
                // Expected: false (port open fails)
                assert!(!ok);
                saw_done.store(true, Ordering::SeqCst);
            }
        });
        assert!(res.is_err());
        assert!(saw_done.load(Ordering::SeqCst), "expected Done progress");
        // Error must NOT be UnknownChip — confirms chip lookup was bypassed.
        match res {
            Err(FlashError::UnknownChip(_)) => {
                panic!("authorize mode must not reach chip registry");
            }
            _ => {}
        }
    }
}
