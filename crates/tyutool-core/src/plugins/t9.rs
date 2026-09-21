//! T9 flash plugin — real hardware implementation.
//!
//! T9 uses the T5AI protocol variant (extended reset sequence, per-sector CRC,
//! skip blank sectors). Reuses the shared Beken driver via
//! [`super::beken::driver::run_beken`] with `is_t5ai=true`.
//!
//! Same family as T3/T1: T5AI handler, not `BK7231NFlashHandler`.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;

use super::beken::chip::T9Spec;

/// T9 flash plugin using the real Beken UART protocol (T5AI variant).
pub struct T9Plugin;

impl FlashPlugin for T9Plugin {
    fn id(&self) -> &'static str {
        "T9"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        let chip = T9Spec;
        super::beken::driver::run_beken(job, cancel, progress, &chip, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(T9Plugin.id(), "T9");
    }
}
