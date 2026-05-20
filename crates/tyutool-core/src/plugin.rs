use std::sync::atomic::AtomicBool;

use crate::error::FlashError;
use crate::flash_event::FlashEvent;
use crate::job::FlashJob;

pub trait FlashPlugin: Send + Sync {
    fn id(&self) -> &'static str;

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError>;
}
