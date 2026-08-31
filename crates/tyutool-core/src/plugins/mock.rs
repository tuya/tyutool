//! Scripted mock chip plugin — drives [`crate::registry::run_job_with`] in tests
//! without any hardware. Behind the `mock-chip` Cargo feature; never compiled
//! into a shipped binary.
//!
//! **What this buys, and what it does not.** A `MockPlugin` never writes a byte
//! to a serial port, so it proves nothing whatsoever about any chip protocol.
//! What it exercises is the orchestration around the plugin call: the event
//! contract (`JobSummary` first, `Done` last), cancellation, the mapping from a
//! plugin error to `Done{Err}`, and the chip-id routing that gets a frontend's
//! job to the right plugin. Coverage numbers will rise when these tests land;
//! protocol assurance will not have moved at all. Protocol-level coverage is a
//! separate mechanism — see `IoTransport` / `MockIo` in `plugins::beken`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::error::FlashError;
use crate::flash_event::{FlashEvent, FlashMilestone, FlashPhase};
use crate::job::FlashJob;
use crate::plugin::FlashPlugin;

/// A plugin's whole job, as a closure. Deliberately the same shape as
/// [`FlashPlugin::run`]: wrapping it in a step/script enum would only add a
/// translation layer over the signature callers already understand.
type MockBehavior = Box<
    dyn Fn(&FlashJob, &AtomicBool, &dyn Fn(FlashEvent)) -> Result<(), FlashError> + Send + Sync,
>;

/// How long [`MockPlugin::blocking_until_cancelled`] waits before giving up.
///
/// A test that forgets to set the cancel flag would otherwise hang until the
/// CI job's own timeout kills it, with no clue as to which test was at fault.
/// Bailing out with a named error turns that into an ordinary failing test.
const CANCEL_WAIT_LIMIT: Duration = Duration::from_secs(5);

/// Wall-clock length of a [`MockPlugin::simulated`] run, and how often it looks
/// at the cancel flag.
///
/// Long enough that a frontend driving it over an IPC boundary can watch
/// progress arrive and still get a cancel in edgewise — which is the whole
/// point of the simulated run — and short enough not to drag a suite out.
const SIMULATED_RUN: Duration = Duration::from_millis(1_500);
const SIMULATED_STEP: Duration = Duration::from_millis(50);

/// A chip plugin whose behaviour is supplied by the test.
pub struct MockPlugin {
    id: &'static str,
    behavior: MockBehavior,
}

impl MockPlugin {
    /// Full form: `behavior` receives exactly what [`FlashPlugin::run`] does.
    pub fn with<F>(id: &'static str, behavior: F) -> Self
    where
        F: Fn(&FlashJob, &AtomicBool, &dyn Fn(FlashEvent)) -> Result<(), FlashError>
            + Send
            + Sync
            + 'static,
    {
        Self {
            id,
            behavior: Box::new(behavior),
        }
    }

    /// Succeeds after emitting a plausible handshake-then-progress sequence.
    pub fn ok(id: &'static str) -> Self {
        Self::with(id, |_job, _cancel, progress| {
            progress(FlashEvent::Phase {
                phase: FlashPhase::Handshake,
            });
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::HandshakeComplete,
            });
            progress(FlashEvent::Percent { value: 100 });
            Ok(())
        })
    }

    /// Fails immediately with `message` as the plugin error text.
    pub fn failing(id: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        Self::with(id, move |_job, _cancel, _progress| {
            Err(FlashError::Plugin(message.clone()))
        })
    }

    /// Walks a full, plausible flash in real time — handshake, flash id, erase,
    /// a `Percent` ramp, verify, reboot — checking the cancel flag throughout.
    ///
    /// This is the variant registered as the `MOCK` chip in
    /// [`crate::registry::FlashPluginRegistry::new`], so every frontend can
    /// drive a job that behaves like hardware without any. Unlike the other
    /// constructors it deliberately **takes time**: a run that returns instantly
    /// cannot be cancelled mid-flight, and "can the user still cancel?" is
    /// exactly the sort of wiring these fakes exist to check.
    pub fn simulated(id: &'static str) -> Self {
        Self::with(id, |_job, cancel, progress| {
            let check = |cancel: &AtomicBool| {
                if cancel.load(Ordering::SeqCst) {
                    return Err(FlashError::Cancelled);
                }
                Ok(())
            };

            progress(FlashEvent::Phase {
                phase: FlashPhase::Handshake,
            });
            check(cancel)?;
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::HandshakeComplete,
            });

            progress(FlashEvent::Phase {
                phase: FlashPhase::ReadFlashId,
            });
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::FlashIdRead {
                    mid: Some(0x00c8_4013),
                },
            });

            progress(FlashEvent::Phase {
                phase: FlashPhase::Erase,
            });
            check(cancel)?;
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::EraseComplete,
            });

            progress(FlashEvent::Phase {
                phase: FlashPhase::Write,
            });
            let steps = (SIMULATED_RUN.as_millis() / SIMULATED_STEP.as_millis()).max(1) as u32;
            for step in 1..=steps {
                std::thread::sleep(SIMULATED_STEP);
                check(cancel)?;
                progress(FlashEvent::Percent {
                    value: (step * 100 / steps) as u8,
                });
            }
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::WriteComplete,
            });

            progress(FlashEvent::Phase {
                phase: FlashPhase::Verify,
            });
            check(cancel)?;
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::VerifyPassed,
            });

            progress(FlashEvent::Phase {
                phase: FlashPhase::Reboot,
            });
            progress(FlashEvent::Milestone {
                milestone: FlashMilestone::Rebooted,
            });
            Ok(())
        })
    }

    /// Spins until the cancel flag is set, then reports [`FlashError::Cancelled`]
    /// the way a real plugin does. See [`CANCEL_WAIT_LIMIT`] for the bail-out.
    pub fn blocking_until_cancelled(id: &'static str) -> Self {
        Self::with(id, |_job, cancel, progress| {
            progress(FlashEvent::Phase {
                phase: FlashPhase::Handshake,
            });
            let start = Instant::now();
            while !cancel.load(Ordering::SeqCst) {
                if start.elapsed() > CANCEL_WAIT_LIMIT {
                    return Err(FlashError::Plugin(
                        "mock: cancel flag was never set".to_string(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(FlashError::Cancelled)
        })
    }
}

impl FlashPlugin for MockPlugin {
    fn id(&self) -> &'static str {
        self.id
    }

    fn run(
        &self,
        job: &FlashJob,
        cancel: &AtomicBool,
        progress: &dyn Fn(FlashEvent),
    ) -> Result<(), FlashError> {
        (self.behavior)(job, cancel, progress)
    }
}
