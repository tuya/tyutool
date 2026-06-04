//! ESP32-P4 flash plugin.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::flash_event::FlashEvent;

use super::esp::chips::ESP32P4_DEF;
use super::esp::common::run_esp;

pub struct Esp32p4Plugin;

impl FlashPlugin for Esp32p4Plugin {
    fn id(&self) -> &'static str {
        "ESP32P4"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        run_esp(job, cancel, progress, &ESP32P4_DEF)
    }
}
