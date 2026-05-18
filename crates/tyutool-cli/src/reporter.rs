use std::sync::Mutex;
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};
use tyutool_core::FlashProgress;

pub struct JobInfo<'a> {
    pub mode: &'a str,
    pub device: &'a str,
    pub port: &'a str,
    pub baud: u32,
    pub file: Option<&'a str>,
    pub file_size: Option<u64>,
    pub range_start: &'a str,
    pub range_end: &'a str,
}

pub struct CliReporter {
    inner: Mutex<Inner>,
}

struct Inner {
    pb: ProgressBar,
    start: Instant,
    current_phase: Option<String>,
    next_milestone: u8,
}

impl CliReporter {
    pub fn new(info: &JobInfo<'_>) -> Self {
        let is_rich = console::Term::stderr().is_term();

        if is_rich {
            eprintln!(
                "tyutool {} · {} · {} @ {}",
                info.mode, info.device, info.port, info.baud
            );
        } else {
            eprintln!(
                "tyutool {}  {}  {}  {}",
                info.mode, info.device, info.port, info.baud
            );
        }
        if let Some(file) = info.file {
            let size_str = info
                .file_size
                .map(|s| format!("  {}", format_file_size(s)))
                .unwrap_or_default();
            eprintln!("  File   {}{}", file, size_str);
        }
        if is_rich {
            eprintln!("  Range  {} → {}", info.range_start, info.range_end);
        } else {
            eprintln!("  Range  {} -> {}", info.range_start, info.range_end);
        }
        eprintln!();

        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::with_template(
                "  {spinner:.cyan} {msg:<14} {bar:25.cyan/black}  {percent:>3}%",
            )
            .unwrap()
            .progress_chars("━━░"),
        );
        pb.enable_steady_tick(std::time::Duration::from_millis(80));

        Self {
            inner: Mutex::new(Inner {
                pb,
                start: Instant::now(),
                current_phase: None,
                next_milestone: 10,
            }),
        }
    }
}

impl Inner {
    fn is_plain(&self) -> bool {
        self.pb.is_hidden()
    }

    fn handle(&mut self, p: FlashProgress) {
        match p {
            FlashProgress::Phase { name } => self.on_phase(name),
            FlashProgress::Percent { value } => self.on_percent(value),
            FlashProgress::Done { ok, message } => self.on_done(ok, message),
            _ => {}
        }
    }

    fn on_phase(&mut self, name: String) {
        let label = map_phase(&name);
        self.finish_current_phase();
        self.current_phase = Some(label.clone());
        self.next_milestone = 10;

        if self.is_plain() {
            eprint!("{:<14}", label);
        } else {
            self.pb.set_position(0);
            self.pb.set_message(label);
        }
    }

    fn finish_current_phase(&mut self) {
        if let Some(label) = self.current_phase.take() {
            if self.is_plain() {
                eprintln!("  OK");
            } else {
                self.pb.println(format!("  \x1b[32m✓\x1b[0m {}", label));
                self.pb.set_position(0);
            }
        }
    }

    fn on_done(&mut self, ok: bool, message: Option<String>) {
        self.finish_current_phase();

        let secs = self.start.elapsed().as_secs_f64();

        if self.is_plain() {
            if ok {
                eprintln!("Flash OK  {:.1}s", secs);
            } else {
                let msg = message.as_deref().unwrap_or("unknown error");
                eprintln!("Flash FAILED: {}  {:.1}s", msg, secs);
            }
        } else {
            self.pb.finish_and_clear();
            if ok {
                eprintln!("  \x1b[32m✓\x1b[0m Flash complete  {:.1}s", secs);
            } else {
                let msg = message.as_deref().unwrap_or("unknown error");
                eprintln!("  \x1b[31m✗\x1b[0m Flash failed: {}  {:.1}s", msg, secs);
            }
        }
    }

    fn on_percent(&mut self, value: u8) {
        let label = match &self.current_phase {
            Some(l) => l.clone(),
            None => return,
        };

        if self.is_plain() {
            if is_long_phase(&label) {
                for m in pop_milestones(&mut self.next_milestone, value) {
                    eprint!("  {}%", m);
                }
            }
        } else {
            self.pb.set_position(value as u64);
        }
    }
}

pub(crate) fn map_phase(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("segment_") {
        if let Some((n, m)) = rest.split_once("_of_") {
            return format!("Write [{}/{}]", n, m);
        }
    }
    match name {
        "ReadFlashID"    => "Flash ID",
        "Unprotect"      => "Unprotect",
        "Protect"        => "Protect",
        "Handshake"      => "Handshake",
        "Erase"          => "Erase",
        "Write"          => "Write",
        "Verify"         => "Verify",
        "Reboot"         => "Reboot",
        "Connect"        => "Connect",
        "connecting"     => "Connect",
        "loading_ram"    => "Load RAM",
        "switching_baud" => "Switch Baud",
        "reading"        => "Read",
        "Read"           => "Read",
        "saving"         => "Save",
        "Save"           => "Save",
        "rebooting"      => "Reboot",
        other            => return other.to_string(),
    }
    .to_string()
}

pub(crate) fn is_long_phase(label: &str) -> bool {
    label == "Write"
        || label == "Erase"
        || label == "Read"
        || label.starts_with("Write [")
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
    while *next_milestone < 100 && value >= *next_milestone {
        out.push(*next_milestone);
        *next_milestone = next_milestone.saturating_add(10);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // format_file_size
    #[test]
    fn file_size_bytes() {
        assert_eq!(format_file_size(512), "512 B");
    }
    #[test]
    fn file_size_kib() {
        assert_eq!(format_file_size(2048), "2.0 KiB");
    }
    #[test]
    fn file_size_mib() {
        assert_eq!(format_file_size(1_887_437), "1.8 MiB");
    }

    // map_phase
    #[test]
    fn phase_known() {
        assert_eq!(map_phase("ReadFlashID"), "Flash ID");
        assert_eq!(map_phase("Handshake"), "Handshake");
        assert_eq!(map_phase("connecting"), "Connect");
        assert_eq!(map_phase("loading_ram"), "Load RAM");
        assert_eq!(map_phase("switching_baud"), "Switch Baud");
        assert_eq!(map_phase("rebooting"), "Reboot");
        assert_eq!(map_phase("saving"), "Save");
        assert_eq!(map_phase("reading"), "Read");
    }
    #[test]
    fn phase_segment() {
        assert_eq!(map_phase("segment_1_of_3"), "Write [1/3]");
        assert_eq!(map_phase("segment_2_of_3"), "Write [2/3]");
    }
    #[test]
    fn phase_unknown_passthrough() {
        assert_eq!(map_phase("SomeNewPhase"), "SomeNewPhase");
    }

    // is_long_phase
    #[test]
    fn long_phases() {
        assert!(is_long_phase("Write"));
        assert!(is_long_phase("Erase"));
        assert!(is_long_phase("Read"));
        assert!(is_long_phase("Write [1/3]"));
    }
    #[test]
    fn short_phases() {
        assert!(!is_long_phase("Handshake"));
        assert!(!is_long_phase("Reboot"));
        assert!(!is_long_phase("Flash ID"));
        assert!(!is_long_phase("Verify"));
    }

    // pop_milestones
    #[test]
    fn milestone_first_crossing() {
        let mut m: u8 = 10;
        assert_eq!(pop_milestones(&mut m, 15), vec![10]);
        assert_eq!(m, 20);
    }
    #[test]
    fn milestone_multiple_crossings() {
        let mut m: u8 = 10;
        assert_eq!(pop_milestones(&mut m, 45), vec![10, 20, 30, 40]);
        assert_eq!(m, 50);
    }
    #[test]
    fn milestone_no_crossing() {
        let mut m: u8 = 10;
        assert_eq!(pop_milestones(&mut m, 5), Vec::<u8>::new());
        assert_eq!(m, 10);
    }
    #[test]
    fn milestone_stops_before_100() {
        let mut m: u8 = 90;
        assert_eq!(pop_milestones(&mut m, 100), vec![90]);
        assert_eq!(m, 100);
    }
}
