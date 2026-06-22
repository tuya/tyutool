//! ESP32-C3 flash plugin.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;

use super::esp::chips::ESP32C3_DEF;
use super::esp::common::run_esp;

pub struct Esp32c3Plugin;

impl FlashPlugin for Esp32c3Plugin {
    fn id(&self) -> &'static str {
        "ESP32C3"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        run_esp(job, cancel, progress, &ESP32C3_DEF)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(Esp32c3Plugin.id(), "ESP32C3");
    }
}
