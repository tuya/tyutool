//! Shared confirmation-gate fakes for the bridge integration tests.
//!
//! The B7 gate is on by default, so a test that drives a dangerous operation
//! (`run_job` / `run_auth`) has to state what the user would have answered.
//! Tests about orchestration use [`approving`]; tests about the gate itself use
//! the refusing / never-answering variants.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tyutool_bridge::{AuditSink, AuthPrompt, ConfirmDecision, ConfirmRequest, ConfirmResponder};

/// Answers "允许" the moment it is asked.
#[derive(Default)]
pub struct ApprovingPrompt {
    seen: Mutex<Vec<ConfirmRequest>>,
}

/// Answers "拒绝" the moment it is asked.
#[derive(Default)]
pub struct RejectingPrompt {
    seen: Mutex<Vec<ConfirmRequest>>,
}

/// Never answers: the user walked away from the dialog. Holds the responder so
/// dropping it cannot be mistaken for an answer.
#[derive(Default)]
pub struct HangingPrompt {
    seen: Mutex<Vec<ConfirmRequest>>,
    kept: Mutex<Vec<ConfirmResponder>>,
}

pub fn approving() -> Arc<ApprovingPrompt> {
    Arc::new(ApprovingPrompt::default())
}

pub fn rejecting() -> Arc<RejectingPrompt> {
    Arc::new(RejectingPrompt::default())
}

pub fn hanging() -> Arc<HangingPrompt> {
    Arc::new(HangingPrompt::default())
}

impl ApprovingPrompt {
    pub fn requests(&self) -> Vec<ConfirmRequest> {
        self.seen.lock().expect("prompt lock").clone()
    }
}

impl RejectingPrompt {
    pub fn requests(&self) -> Vec<ConfirmRequest> {
        self.seen.lock().expect("prompt lock").clone()
    }
}

impl HangingPrompt {
    pub fn requests(&self) -> Vec<ConfirmRequest> {
        self.seen.lock().expect("prompt lock").clone()
    }

    /// Wait until the gate raised its first prompt (or give up).
    pub async fn first_request(&self, within: Duration) -> ConfirmRequest {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(first) = self.requests().into_iter().next() {
                return first;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no confirmation was requested within {within:?}");
    }
}

impl AuthPrompt for ApprovingPrompt {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        self.seen.lock().expect("prompt lock").push(request);
        respond(ConfirmDecision::Approve);
    }
}

impl AuthPrompt for RejectingPrompt {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        self.seen.lock().expect("prompt lock").push(request);
        respond(ConfirmDecision::Reject);
    }
}

impl AuthPrompt for HangingPrompt {
    fn request(&self, request: ConfirmRequest, respond: ConfirmResponder) {
        self.seen.lock().expect("prompt lock").push(request);
        self.kept.lock().expect("responder lock").push(respond);
    }
}

/// Collects audit lines so a test can grep them.
#[derive(Default)]
pub struct CapturingAudit {
    lines: Mutex<Vec<String>>,
}

pub fn capturing_audit() -> Arc<CapturingAudit> {
    Arc::new(CapturingAudit::default())
}

impl CapturingAudit {
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("audit lock").clone()
    }
}

impl AuditSink for CapturingAudit {
    fn record(&self, line: &str) {
        self.lines
            .lock()
            .expect("audit lock")
            .push(line.to_string());
    }
}
