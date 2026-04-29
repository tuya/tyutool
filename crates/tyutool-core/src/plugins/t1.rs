//! T1 flash plugin — real hardware implementation.
//!
//! Reuses the shared Beken protocol layer via [`super::bk7231n::run_beken`],
//! with `T1Spec` matching the T5 extended-frame / per-sector CRC behaviour.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use super::beken::chip::T1Spec;

/// T1 flash plugin using the real Beken UART protocol (T5-equivalent stack).
pub struct T1Plugin;

impl FlashPlugin for T1Plugin {
    fn id(&self) -> &'static str {
        "T1"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        log::info!("T1 plugin delegating to run_beken (is_t5=true)");
        let chip = T1Spec;
        super::bk7231n::run_beken(job, cancel, progress, &chip, true)
    }
}
