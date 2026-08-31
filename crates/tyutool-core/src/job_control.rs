//! The two pieces of per-job control that every frontend needs and none of them
//! should own: the cancel flag's lifecycle, and the overwrite-confirmation
//! handshake.
//!
//! Deliberately *not* here: threads, tasks, event sinks, and the policy for what
//! to do when a job is already running. `src-tauri` runs jobs on a
//! `std::thread` and waits for the previous one; `tyutool-serve` runs them on a
//! tokio task and refuses a second. Those differences are real and reasonable,
//! and folding them into one abstraction would cost more than the duplication
//! it removed. What is shared is the *semantics* below, nothing else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Hands out the cancel flag for one job at a time.
///
/// The point of the type is a single invariant: **starting a new job can never
/// clear the flag an old job is still watching.** [`begin`](Self::begin) does
/// not reset the existing flag, it replaces it — the outgoing `Arc` is left
/// `true` forever, and the job holding it keeps seeing the cancel it was given.
///
/// That matters because the alternative — one shared flag, reset at the start of
/// each job — is only safe while something *elsewhere* forbids two jobs from
/// overlapping. `tyutool-serve` was in exactly that position: correct, but
/// correct because of a check in another function rather than because of
/// anything the data structure guaranteed. Here the guarantee is local.
#[derive(Debug)]
pub struct CancelSlot {
    current: Mutex<Arc<AtomicBool>>,
}

impl CancelSlot {
    pub fn new() -> Self {
        Self {
            current: Mutex::new(Arc::new(AtomicBool::new(false))),
        }
    }

    /// Signal whatever is running to stop, and return a fresh flag for the new
    /// job. The swap happens under the lock so the two flags are never the same
    /// `Arc`, however the calls interleave.
    pub fn begin(&self) -> Arc<AtomicBool> {
        let fresh = Arc::new(AtomicBool::new(false));
        let previous = {
            let mut guard = self.lock();
            std::mem::replace(&mut *guard, Arc::clone(&fresh))
        };
        previous.store(true, Ordering::SeqCst);
        fresh
    }

    /// Signal the job currently in flight to stop.
    pub fn cancel(&self) {
        self.lock().store(true, Ordering::SeqCst);
    }

    /// The flag of the job currently in flight, for a caller that needs to hand
    /// it to something after the job has already begun.
    pub fn current(&self) -> Arc<AtomicBool> {
        Arc::clone(&*self.lock())
    }

    /// A poisoned lock here means some other thread panicked while holding it.
    /// The flag behind it is an `AtomicBool` with no invariant to corrupt, so
    /// recovering is strictly better than propagating a panic into a flash job.
    fn lock(&self) -> std::sync::MutexGuard<'_, Arc<AtomicBool>> {
        self.current.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for CancelSlot {
    fn default() -> Self {
        Self::new()
    }
}

/// The overwrite-confirmation handshake: a blocking job thread asks, and
/// whatever frontend owns the user answers.
///
/// `tyutool-core`'s authorize flow calls [`ask`](Self::ask) from inside
/// `FlashJob::confirm_overwrite` and blocks there. The frontend shows whatever
/// it shows — a Tauri dialog, a WebSocket frame — and then calls
/// [`resolve`](Self::resolve). What gets shown is not this type's business.
#[derive(Debug)]
pub struct ConfirmSlot {
    pending: Mutex<Option<mpsc::Sender<bool>>>,
}

impl ConfirmSlot {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(None),
        }
    }

    /// Blocks until [`resolve`](Self::resolve) or [`clear`](Self::clear) is
    /// called. Answers `false` if the sender is dropped, so a frontend that
    /// disappears mid-prompt declines rather than parking the job thread on a
    /// serial port forever.
    pub fn ask(&self) -> bool {
        let (tx, rx) = mpsc::channel::<bool>();
        *self.lock() = Some(tx);
        rx.recv().unwrap_or(false)
    }

    /// Answer a pending ask. Returns whether there was one.
    ///
    /// The return value is not decoration: `src-tauri` turns `false` into an
    /// error, because a confirm command that silently succeeded with nothing
    /// pending would let the frontend believe a dialog had been dealt with.
    pub fn resolve(&self, answer: bool) -> bool {
        match self.lock().take() {
            Some(tx) => {
                let _ = tx.send(answer);
                true
            }
            None => false,
        }
    }

    /// Drop a stale pending ask left behind by a run that exited abnormally.
    /// The waiter, if any, sees `false`.
    pub fn clear(&self) {
        let _ = self.lock().take();
    }

    /// See [`CancelSlot::lock`] for why a poisoned lock is recovered rather
    /// than propagated.
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<mpsc::Sender<bool>>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for ConfirmSlot {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn begin_leaves_the_previous_flag_cancelled() {
        let slot = CancelSlot::new();
        let first = slot.begin();
        assert!(!first.load(Ordering::SeqCst));

        let second = slot.begin();

        // The whole reason this type exists: the job holding `first` still sees
        // the cancel it was given, even though a new job has started.
        assert!(
            first.load(Ordering::SeqCst),
            "starting a new job must cancel the old one, not un-cancel it",
        );
        assert!(!second.load(Ordering::SeqCst));
        assert!(
            !Arc::ptr_eq(&first, &second),
            "two jobs must never share a cancel flag",
        );
    }

    #[test]
    fn cancel_reaches_the_job_in_flight() {
        let slot = CancelSlot::new();
        let running = slot.begin();
        slot.cancel();
        assert!(running.load(Ordering::SeqCst));
        assert!(slot.current().load(Ordering::SeqCst));
    }

    #[test]
    fn cancelling_before_any_job_does_not_leak_into_the_next_one() {
        let slot = CancelSlot::new();
        // A cancel arriving with nothing running (a stray click, a disconnect)
        // must not pre-cancel whatever starts next.
        slot.cancel();
        assert!(!slot.begin().load(Ordering::SeqCst));
    }

    #[test]
    fn resolve_reports_whether_anything_was_pending() {
        let slot = ConfirmSlot::new();
        assert!(!slot.resolve(true), "nothing was pending");
    }

    #[test]
    fn ask_returns_the_answer_it_was_given() {
        let slot = Arc::new(ConfirmSlot::new());
        let asker = Arc::clone(&slot);
        let handle = std::thread::spawn(move || asker.ask());

        // `ask` registers its sender before blocking; retry until it appears
        // rather than sleeping a guessed amount.
        loop {
            if slot.resolve(true) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(handle.join().expect("asker thread"));
    }

    #[test]
    fn clear_releases_a_waiter_with_a_refusal() {
        let slot = Arc::new(ConfirmSlot::new());
        let asker = Arc::clone(&slot);
        let handle = std::thread::spawn(move || asker.ask());

        loop {
            {
                let mut guard = slot.lock();
                if guard.is_some() {
                    let _ = guard.take();
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }

        // Dropping the sender must unblock the job thread as a decline, not
        // leave it holding the serial port forever.
        assert!(!handle.join().expect("asker thread"));
    }
}
