//! T5 flash plugin — real hardware implementation.
//!
//! Reuses the shared Beken protocol layer via [`super::bk7231n::run_beken`],
//! with `T5Spec` providing the T5-specific behaviour differences.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use super::beken::chip::T5Spec;

/// T5 flash plugin using the real Beken UART protocol.
pub struct T5Plugin;

impl FlashPlugin for T5Plugin {
    fn id(&self) -> &'static str {
        "T5"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        log::info!("T5 plugin delegating to run_beken (is_t5=true)");
        let chip = T5Spec;
        super::bk7231n::run_beken(job, cancel, progress, &chip, true)
    }
}
