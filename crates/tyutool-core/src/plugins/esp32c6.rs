//! ESP32-C6 flash plugin.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use super::esp::chips::ESP32C6_DEF;
use super::esp::common::run_esp;

pub struct Esp32c6Plugin;

impl FlashPlugin for Esp32c6Plugin {
    fn id(&self) -> &'static str {
        "ESP32C6"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        run_esp(job, cancel, progress, &ESP32C6_DEF)
    }
}
