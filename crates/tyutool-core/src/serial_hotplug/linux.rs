use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc,
};

// inotify observes devfs creation/removal and udev permission changes. This also
// works in musl CLI builds without linking libudev. Reads/open/close are ignored.
fn watch(directory: &Path) -> io::Result<OwnedFd> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(directory.as_os_str().as_bytes())?;
    // SAFETY: inotify_init1 returns a new fd, owned exactly once below.
    let raw = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    // SAFETY: path is NUL-terminated and fd is a live inotify instance.
    if unsafe {
        libc::inotify_add_watch(
            raw,
            path.as_ptr(),
            libc::IN_CREATE
                | libc::IN_DELETE
                | libc::IN_ATTRIB
                | libc::IN_MOVED_FROM
                | libc::IN_MOVED_TO,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(fd)
}

fn serial_event(bytes: &[u8]) -> bool {
    let header = std::mem::size_of::<libc::inotify_event>();
    let mut offset = 0;
    while offset + header <= bytes.len() {
        // SAFETY: header length checked; kernel event buffer may be unaligned.
        let event = unsafe {
            std::ptr::read_unaligned(bytes[offset..].as_ptr().cast::<libc::inotify_event>())
        };
        let end = offset + header + event.len as usize;
        if end > bytes.len() {
            break;
        }
        if event.mask & libc::IN_Q_OVERFLOW != 0 {
            return true;
        }
        let name = &bytes[offset + header..end];
        if name.starts_with(b"tty") || name.starts_with(b"rfcomm") {
            return true;
        }
        offset = end;
    }
    false
}

pub(super) fn run(changed: impl Fn(), stop: Arc<AtomicBool>, ready: SyncSender<io::Result<()>>) {
    let fd = match watch(Path::new("/dev")) {
        Ok(fd) => fd,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let mut buffer = [0u8; 8192];
    let mut pollfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    while !stop.load(Ordering::Relaxed) {
        // The timeout only checks shutdown; enumeration is event-driven.
        let status = unsafe { libc::poll(&mut pollfd, 1, 250) };
        if status < 0 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            log::warn!(
                "Serial device notification poll failed: {}",
                io::Error::last_os_error()
            );
            break;
        }
        if status == 0 {
            continue;
        }
        let count = unsafe { libc::read(fd.as_raw_fd(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if count > 0 && serial_event(&buffer[..count as usize]) {
            changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observes_serial_device_creation_and_removal_without_enumerating() {
        let directory = tempfile::tempdir().expect("test directory");
        let fd = watch(directory.path()).expect("inotify watch");
        let serial = directory.path().join("ttyUSB-test");
        std::fs::write(&serial, []).expect("create serial node fixture");
        let mut bytes = [0u8; 1024];
        let read = |bytes: &mut [u8]| unsafe {
            libc::read(fd.as_raw_fd(), bytes.as_mut_ptr().cast(), bytes.len())
        };
        // Kernel queues inotify records synchronously with the file operations.
        let n = read(&mut bytes);
        assert!(n > 0);
        assert!(serial_event(&bytes[..n as usize]));
        std::fs::remove_file(serial).expect("remove fixture");
        let n = read(&mut bytes);
        assert!(n > 0);
        assert!(serial_event(&bytes[..n as usize]));
        std::fs::write(directory.path().join("unrelated"), []).expect("unrelated fixture");
        let n = read(&mut bytes);
        assert!(n > 0);
        assert!(!serial_event(&bytes[..n as usize]));
    }
}
