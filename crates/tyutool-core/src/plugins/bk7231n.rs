//! BK7231N flash plugin — real hardware implementation.
//!
//! Reuses the shared Beken driver via [`super::beken::driver::run_beken`],
//! with `Bk7231nSpec` providing the BK7231N-specific behaviour.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;

use super::beken::chip::Bk7231nSpec;

/// BK7231N flash plugin using the real Beken UART protocol.
pub struct Bk7231nPlugin;

impl FlashPlugin for Bk7231nPlugin {
    fn id(&self) -> &'static str {
        "BK7231N"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        let chip = Bk7231nSpec;
        super::beken::driver::run_beken(job, cancel, progress, &chip, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(Bk7231nPlugin.id(), "BK7231N");
    }
}
