use core_foundation_sys::runloop::{
    kCFRunLoopDefaultMode, CFRunLoopAddSource, CFRunLoopGetCurrent, CFRunLoopRemoveSource,
    CFRunLoopRunInMode,
};
use io_kit_sys::types::*;
use io_kit_sys::*;
use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc,
};

pub(super) fn run<F: Fn()>(changed: F, stop: Arc<AtomicBool>, ready: SyncSender<io::Result<()>>) {
    unsafe extern "C" fn notify<F: Fn()>(context: *mut std::ffi::c_void, iterator: io_iterator_t) {
        let mut found = false;
        loop {
            let service = unsafe { IOIteratorNext(iterator) };
            if service == 0 {
                break;
            }
            unsafe {
                IOObjectRelease(service);
            }
            found = true;
        }
        // Draining the iterator re-arms the notification.
        if found {
            unsafe {
                (&*context.cast::<F>())();
            }
        }
    }
    let changed = Box::new(changed);
    unsafe {
        let port = IONotificationPortCreate(0);
        if port.is_null() {
            let _ = ready.send(Err(io::Error::other("IONotificationPortCreate failed")));
            return;
        }
        let source = IONotificationPortGetRunLoopSource(port);
        let runloop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(runloop, source, kCFRunLoopDefaultMode);
        let mut iterators = [0; 2];
        let mut failed = None;
        for (index, kind) in [c"IOServiceFirstMatch", c"IOServiceTerminate"]
            .iter()
            .enumerate()
        {
            // IOSerialBSDClient matches USB, Bluetooth and built-in serial ports.
            // IOKit consumes the matching dictionary on every registration.
            let matching = IOServiceMatching(c"IOSerialBSDClient".as_ptr());
            let result = IOServiceAddMatchingNotification(
                port,
                kind.as_ptr().cast_mut(),
                matching,
                notify::<F>,
                (&*changed as *const F).cast_mut().cast(),
                &mut iterators[index],
            );
            if result != 0 {
                failed = Some(result);
                break;
            }
            // Initial matches arm the watcher without synthesizing a hotplug.
            loop {
                let service = IOIteratorNext(iterators[index]);
                if service == 0 {
                    break;
                }
                IOObjectRelease(service);
            }
        }
        let _ = ready.send(match failed {
            Some(code) => Err(io::Error::other(format!(
                "IOServiceAddMatchingNotification: {code}"
            ))),
            None => Ok(()),
        });
        if failed.is_none() {
            while !stop.load(Ordering::Relaxed) {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.25, 0);
            }
        }
        CFRunLoopRemoveSource(runloop, source, kCFRunLoopDefaultMode);
        for iterator in iterators {
            if iterator != 0 {
                IOObjectRelease(iterator);
            }
        }
        IONotificationPortDestroy(port);
    }
}
