use std::sync::Mutex;

use indicatif::{ProgressBar, ProgressStyle};
use tyutool_core::{
    FlashEvent, FlashMilestone, FlashPhase, FlashResult, JobDetails, JobSummary,
};

pub struct CliReporter {
    inner: Mutex<Inner>,
}

struct Inner {
    pb: ProgressBar,
    is_plain: bool,
    current_phase_label: Option<String>,
    next_milestone: u8,
    inline: bool,       // phase label printed but no newline yet (plain mode)
    show_percent: bool, // current phase emits Percent events
}

impl CliReporter {
    pub fn new(force_plain: bool) -> Self {
        let is_plain = force_plain || !console::Term::stderr().is_term();

        let pb = ProgressBar::new(100);
        if is_plain {
            pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
        } else {
            pb.set_style(
                ProgressStyle::with_template(
                    "  {spinner:.cyan} {msg:<16} {bar:25.cyan/black}  {percent:>3}%",
                )
                .unwrap()
                .progress_chars("━━░"),
            );
            pb.enable_steady_tick(std::time::Duration::from_millis(80));
        }

        Self {
            inner: Mutex::new(Inner {
                pb,
                is_plain,
                current_phase_label: None,
                next_milestone: 10,
                inline: false,
                show_percent: false,
            }),
        }
    }

    pub fn callback(&self) -> impl Fn(FlashEvent) + '_ {
        move |e| self.inner.lock().unwrap().handle(e)
    }
}

#[cfg(test)]
impl CliReporter {
    pub fn is_plain(&self) -> bool {
        self.inner.lock().unwrap().is_plain
    }

    pub fn is_inline(&self) -> bool {
        self.inner.lock().unwrap().inline
    }

    pub fn show_percent_flag(&self) -> bool {
        self.inner.lock().unwrap().show_percent
    }
}

impl Inner {
    fn handle(&mut self, e: FlashEvent) {
        match e {
            FlashEvent::JobSummary(s) => self.on_job_summary(s),
            FlashEvent::Phase { phase } => self.on_phase(phase),
            FlashEvent::Percent { value } => self.on_percent(value),
            FlashEvent::Milestone { milestone } => self.on_milestone(milestone),
            FlashEvent::Warning { message } => self.on_warning(message),
            FlashEvent::Done { result } => self.on_done(result),
        }
    }

    fn on_job_summary(&mut self, s: JobSummary) {
        let sep = if self.is_plain { "->" } else { "→" };

        match &s.details {
            JobDetails::Flash {
                firmware_path,
                firmware_size,
                range_start,
                range_end,
            } => {
                let device = s.device.as_deref().unwrap_or("?");
                if self.is_plain {
                    eprintln!("write  {}  {}  {}", device, s.port, s.baud);
                } else {
                    eprintln!("write · {} · {} @ {}", device, s.port, s.baud);
                }
                let size_str = firmware_size
                    .map(|b| format!("  {}", format_file_size(b)))
                    .unwrap_or_default();
                eprintln!("  File   {}{}", firmware_path, size_str);
                eprintln!("  Range  {} {} {}", range_start, sep, range_end);
            }
            JobDetails::Read {
                output_path,
                range_start,
                range_end,
            } => {
                let device = s.device.as_deref().unwrap_or("?");
                if self.is_plain {
                    eprintln!("read  {}  {}  {}", device, s.port, s.baud);
                } else {
                    eprintln!("read · {} · {} @ {}", device, s.port, s.baud);
                }
                eprintln!("  Output {}", output_path);
                eprintln!("  Range  {} {} {}", range_start, sep, range_end);
            }
            JobDetails::Erase { range_start, range_end } => {
                let device = s.device.as_deref().unwrap_or("?");
                if self.is_plain {
                    eprintln!("erase  {}  {}  {}", device, s.port, s.baud);
                } else {
                    eprintln!("erase · {} · {} @ {}", device, s.port, s.baud);
                }
                eprintln!("  Range  {} {} {}", range_start, sep, range_end);
            }
            JobDetails::Authorize { write } => {
                let mode = if *write { "write" } else { "read-only" };
                if self.is_plain {
                    eprintln!("authorize  {}  {}  [{}]", s.port, s.baud, mode);
                } else {
                    eprintln!("authorize · {} @ {}  [{}]", s.port, s.baud, mode);
                }
            }
        }
        eprintln!();
    }

    fn on_phase(&mut self, phase: FlashPhase) {
        let label = phase_label(&phase);
        let show_percent = is_percent_phase(&phase);
        self.finish_current_phase();
        self.current_phase_label = Some(label.clone());
        self.next_milestone = 10;
        self.show_percent = show_percent;

        if self.is_plain {
            if self.show_percent {
                eprintln!("{}", label);
            } else {
                eprint!("{:<16}", label);
                self.inline = true;
            }
        } else {
            self.pb.set_position(0);
            self.pb.set_message(label);
        }
    }

    fn finish_current_phase(&mut self) {
        if let Some(label) = self.current_phase_label.take() {
            if self.is_plain {
                if self.show_percent {
                    eprintln!("  100%");
                } else {
                    eprintln!("  OK");
                }
                self.inline = false;
            } else {
                self.pb.println(format!("  \x1b[32m✓\x1b[0m {}", label));
                self.pb.set_position(0);
            }
        }
    }

    fn close_inline(&mut self) {
        if self.inline {
            eprintln!();
            self.inline = false;
        }
    }

    fn on_percent(&mut self, value: u8) {
        if self.current_phase_label.is_none() {
            return;
        }

        if self.is_plain {
            if self.show_percent {
                let milestones = pop_milestones(&mut self.next_milestone, value);
                for m in milestones {
                    eprintln!("  {}%", m);
                }
            }
        } else {
            self.pb.set_position(value as u64);
        }
    }

    fn on_milestone(&mut self, milestone: FlashMilestone) {
        let text = milestone_text(&milestone);
        if self.is_plain {
            self.close_inline();
            eprintln!("[OK] {}", text);
        } else {
            self.pb.println(format!("  \x1b[32m✓\x1b[0m {}", text));
        }

        // AuthReadComplete: print credentials on their own lines.
        // CLI shows plainly; GUI handles via secure modal.
        if let FlashMilestone::AuthReadComplete { uuid, authkey } = &milestone {
            if self.is_plain {
                eprintln!("  UUID:    {}", uuid);
                eprintln!("  AuthKey: {}", authkey);
            } else {
                self.pb.println(format!("  UUID:    {}", uuid));
                self.pb.println(format!("  AuthKey: {}", authkey));
            }
        }
    }

    fn on_warning(&mut self, message: String) {
        if self.is_plain {
            self.close_inline();
            eprintln!("[WARN] {}", message);
        } else {
            self.pb.println(format!("  \x1b[33m⚠\x1b[0m {}", message));
        }
    }

    fn on_done(&mut self, result: FlashResult) {
        self.finish_current_phase();

        if self.is_plain {
            match result {
                FlashResult::Ok { elapsed_secs } => {
                    eprintln!("Flash OK  {:.1}s", elapsed_secs);
                }
                FlashResult::Err { message, elapsed_secs } => {
                    eprintln!("Flash FAILED: {}  {:.1}s", message, elapsed_secs);
                }
                FlashResult::Cancelled { elapsed_secs } => {
                    eprintln!("Flash CANCELLED  {:.1}s", elapsed_secs);
                }
            }
        } else {
            self.pb.finish_and_clear();
            match result {
                FlashResult::Ok { elapsed_secs } => {
                    eprintln!("  \x1b[32m✓\x1b[0m Flash complete  {:.1}s", elapsed_secs);
                }
                FlashResult::Err { message, elapsed_secs } => {
                    eprintln!(
                        "  \x1b[31m✗\x1b[0m Flash failed: {}  {:.1}s",
                        message, elapsed_secs
                    );
                }
                FlashResult::Cancelled { elapsed_secs } => {
                    eprintln!("  \x1b[33m✗\x1b[0m Cancelled  {:.1}s", elapsed_secs);
                }
            }
        }
    }
}

pub(crate) fn phase_label(phase: &FlashPhase) -> String {
    match phase {
        FlashPhase::Handshake => "Handshake".into(),
        FlashPhase::ReadFlashId => "Flash ID".into(),
        FlashPhase::Unprotect => "Unprotect".into(),
        FlashPhase::Erase => "Erase".into(),
        FlashPhase::WriteSegment { current, total } => format!("Write [{}/{}]", current, total),
        FlashPhase::Write => "Write".into(),
        FlashPhase::Verify => "Verify".into(),
        FlashPhase::Protect => "Protect".into(),
        FlashPhase::Reboot => "Reboot".into(),
        FlashPhase::Read => "Read".into(),
        FlashPhase::Save => "Save".into(),
        FlashPhase::LoadRam => "Load RAM".into(),
        FlashPhase::SwitchBaud => "Switch Baud".into(),
        FlashPhase::Connect => "Connect".into(),
        FlashPhase::Other(s) => s.clone(),
    }
}

fn milestone_text(m: &FlashMilestone) -> String {
    match m {
        FlashMilestone::HandshakeComplete => "Handshake complete".into(),
        FlashMilestone::Connected { chip_info: Some(info) } => format!("Connected: {}", info),
        FlashMilestone::Connected { chip_info: None } => "Connected".into(),
        FlashMilestone::FlashIdRead { mid: Some(mid) } => format!("Flash ID: {:#010x}", mid),
        FlashMilestone::FlashIdRead { mid: None } => "Flash ID read".into(),
        FlashMilestone::EraseComplete => "Erase complete".into(),
        FlashMilestone::SegmentWritten { current, total } => {
            format!("Segment {}/{} written", current, total)
        }
        FlashMilestone::WriteComplete => "Write complete".into(),
        FlashMilestone::VerifyPassed => "Verify passed".into(),
        FlashMilestone::Rebooted => "Device rebooted".into(),
        FlashMilestone::AuthReadComplete { .. } => "Auth read complete".into(),
        FlashMilestone::AuthReadEmpty => "No authorization found on device".into(),
    }
}

pub(crate) fn is_percent_phase(phase: &FlashPhase) -> bool {
    matches!(
        phase,
        FlashPhase::Write
            | FlashPhase::Erase
            | FlashPhase::Read
            | FlashPhase::WriteSegment { .. }
    )
}

pub(crate) fn format_file_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    }
}

pub(crate) fn pop_milestones(next_milestone: &mut u8, value: u8) -> Vec<u8> {
    let mut out = Vec::new();
    while *next_milestone <= 100 && value >= *next_milestone {
        out.push(*next_milestone);
        *next_milestone = next_milestone.saturating_add(10);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_label_write_segment() {
        let label = phase_label(&FlashPhase::WriteSegment { current: 2, total: 3 });
        assert_eq!(label, "Write [2/3]");
    }

    #[test]
    fn phase_label_known_phases() {
        assert_eq!(phase_label(&FlashPhase::Handshake), "Handshake");
        assert_eq!(phase_label(&FlashPhase::LoadRam), "Load RAM");
        assert_eq!(phase_label(&FlashPhase::SwitchBaud), "Switch Baud");
        assert_eq!(phase_label(&FlashPhase::Other("NewPhase".into())), "NewPhase");
    }

    #[test]
    fn is_percent_phase_detection() {
        assert!(is_percent_phase(&FlashPhase::Write));
        assert!(is_percent_phase(&FlashPhase::Erase));
        assert!(is_percent_phase(&FlashPhase::Read));
        assert!(is_percent_phase(&FlashPhase::WriteSegment { current: 1, total: 3 }));
        assert!(!is_percent_phase(&FlashPhase::Handshake));
        assert!(!is_percent_phase(&FlashPhase::Verify));
    }

    #[test]
    fn pop_milestones_multiple() {
        let mut m: u8 = 10;
        assert_eq!(pop_milestones(&mut m, 35), vec![10, 20, 30]);
        assert_eq!(m, 40);
    }

    #[test]
    fn pop_milestones_includes_100() {
        let mut m: u8 = 90;
        assert_eq!(pop_milestones(&mut m, 100), vec![90, 100]);
        assert_eq!(m, 110);
    }

    #[test]
    fn format_file_size_variants() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(2048), "2.0 KiB");
        assert_eq!(format_file_size(1_887_437), "1.8 MiB");
    }

    #[test]
    fn force_plain_overrides_tty_detection() {
        let reporter = CliReporter::new(true);
        assert!(reporter.is_plain());
    }

    // -----------------------------------------------------------------------
    // plain-mode on_phase: percent phases must NOT be inline (label gets its
    // own newline so pipe readers receive it immediately); non-percent phases
    // MUST stay inline so that "Handshake         OK" lands on one line.
    // -----------------------------------------------------------------------

    #[test]
    fn plain_erase_phase_not_inline() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Erase });
        assert!(!r.is_inline(), "Erase is a percent phase — must NOT be inline");
        assert!(r.show_percent_flag());
    }

    #[test]
    fn plain_write_phase_not_inline() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Write });
        assert!(!r.is_inline(), "Write is a percent phase — must NOT be inline");
        assert!(r.show_percent_flag());
    }

    #[test]
    fn plain_read_phase_not_inline() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Read });
        assert!(!r.is_inline(), "Read is a percent phase — must NOT be inline");
    }

    #[test]
    fn plain_handshake_phase_is_inline() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Handshake });
        assert!(r.is_inline(), "Handshake is non-percent — must be inline");
        assert!(!r.show_percent_flag());
    }

    #[test]
    fn plain_protect_phase_is_inline() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Protect });
        assert!(r.is_inline(), "Protect is non-percent — must be inline");
    }

    #[test]
    fn plain_reboot_phase_is_inline() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Reboot });
        assert!(r.is_inline(), "Reboot is non-percent — must be inline");
    }

    // When a percent phase finishes, show_percent stays true so
    // finish_current_phase knows to emit "100%" instead of "OK".
    #[test]
    fn percent_phase_show_percent_flag_remains_after_phase_start() {
        let r = CliReporter::new(true);
        let cb = r.callback();
        cb(FlashEvent::Phase { phase: FlashPhase::Erase });
        assert!(r.show_percent_flag(), "show_percent must be true while Erase is active");
        // Starting next phase calls finish_current_phase (which reads show_percent),
        // then resets show_percent for the new phase.
        cb(FlashEvent::Phase { phase: FlashPhase::Handshake });
        assert!(!r.show_percent_flag(), "show_percent must be false for Handshake");
    }

    // Milestones are emitted one by one at each 10% boundary.
    #[test]
    fn pop_milestones_one_at_a_time() {
        let mut m: u8 = 10;
        assert_eq!(pop_milestones(&mut m, 10), vec![10]);
        assert_eq!(m, 20);
        assert_eq!(pop_milestones(&mut m, 15), Vec::<u8>::new());
        assert_eq!(m, 20);
        assert_eq!(pop_milestones(&mut m, 20), vec![20]);
        assert_eq!(m, 30);
    }
}
