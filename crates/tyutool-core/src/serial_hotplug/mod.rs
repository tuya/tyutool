//! Native device notifications. No periodic serial-port enumeration.
//! Consumers own the guard and decide when to enumerate after a notification.

use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub struct SerialPortWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl SerialPortWatcher {
    pub fn start(changed: impl Fn() + Send + Sync + 'static) -> io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (ready, result) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("serial-hotplug".into())
            .spawn(move || {
                #[cfg(target_os = "linux")]
                linux::run(changed, worker_stop, ready);
                #[cfg(target_os = "macos")]
                macos::run(changed, worker_stop, ready);
                #[cfg(target_os = "windows")]
                windows::run(changed, worker_stop, ready);
                #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
                {
                    let _ = (changed, worker_stop);
                    let _ = ready.send(Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "serial device notifications unsupported",
                    )));
                }
            })?;
        match result.recv().map_err(io::Error::other)? {
            Ok(()) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }
}

impl Drop for SerialPortWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
