use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::job::FlashJob;
use crate::progress::FlashProgress;

/// One chip-family implementation (Python `FlashHandler` equivalent).
pub trait FlashPlugin: Send + Sync {
    /// Registry id, uppercase (e.g. `BK7231N`, `T5`).
    fn id(&self) -> &'static str;

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashProgress),
    ) -> Result<(), FlashError>;
}
