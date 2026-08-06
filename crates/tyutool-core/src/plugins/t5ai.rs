//! T5AI flash plugin — real hardware implementation.
//!
//! Reuses the shared Beken driver via [`super::beken::driver::run_beken`],
//! with `T5AISpec` providing the T5AI-specific behaviour differences.

use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;

use super::beken::chip::T5AISpec;

/// T5AI flash plugin using the real Beken UART protocol.
pub struct T5AIPlugin;

impl FlashPlugin for T5AIPlugin {
    fn id(&self) -> &'static str {
        "T5AI"
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        let chip = T5AISpec;
        super::beken::driver::run_beken(job, cancel, progress, &chip, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_is_uppercase() {
        assert_eq!(T5AIPlugin.id(), "T5AI");
    }
}
