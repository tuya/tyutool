//! T2 flash plugin — real hardware implementation.
//!
//! T2 uses the standard Beken UART protocol (same frame format as BK7231N),
//! backed by [`T2Spec`] which provides T2-specific chip parameters.
//! The Python reference (`FlashInterface.SocList`) confirms T2 maps to
//! `BK7231NFlashHandler` (standard frames), not `T5FlashHandler`.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;
use crate::progress::FlashProgress;

use super::beken::chip::T2Spec;

/// T2 flash plugin using the real Beken UART protocol (standard frame variant).
pub struct T2Plugin;

impl FlashPlugin for T2Plugin {
    fn id(&self) -> &'static str {
        "T2"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError> {
        let chip = T2Spec;
        super::bk7231n::run_beken(job, cancel, progress, &chip, false)
    }
}
