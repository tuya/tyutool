//! ESP32-S3 flash plugin.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use super::esp::chips::ESP32S3_DEF;
use super::esp::common::run_esp;

pub struct Esp32s3Plugin;

impl FlashPlugin for Esp32s3Plugin {
    fn id(&self) -> &'static str {
        "ESP32S3"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        run_esp(job, cancel, progress, &ESP32S3_DEF)
    }
}
