//! T3 flash plugin — real hardware implementation.
//!
//! T3 uses the T5 protocol variant (extended reset sequence, per-sector CRC,
//! skip blank sectors). Reuses the shared Beken protocol layer via
//! [`super::bk7231n::run_beken`] with `is_t5=true`.
//!
//! The Python reference (`FlashInterface.SocList`) confirms T3 maps to
//! `T5FlashHandler`, not `BK7231NFlashHandler`.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use super::beken::chip::T3Spec;

/// T3 flash plugin using the real Beken UART protocol (T5 variant).
pub struct T3Plugin;

impl FlashPlugin for T3Plugin {
    fn id(&self) -> &'static str {
        "T3"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        log::info!("T3 plugin delegating to run_beken (is_t5=true)");
        let chip = T3Spec;
        super::bk7231n::run_beken(job, cancel, progress, &chip, true)
    }
}
