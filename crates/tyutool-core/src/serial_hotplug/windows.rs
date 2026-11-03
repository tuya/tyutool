use std::io;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::SyncSender,
    Arc,
};
use std::time::Duration;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;

pub(super) fn run<F: Fn() + Send + Sync>(
    changed: F,
    stop: Arc<AtomicBool>,
    ready: SyncSender<io::Result<()>>,
) {
    unsafe extern "system" fn notify<F: Fn()>(
        _: HCMNOTIFICATION,
        context: *const std::ffi::c_void,
        action: CM_NOTIFY_ACTION,
        _: *const CM_NOTIFY_EVENT_DATA,
        _: u32,
    ) -> u32 {
        if action == CM_NOTIFY_ACTION_DEVICEINTERFACEARRIVAL
            || action == CM_NOTIFY_ACTION_DEVICEINTERFACEREMOVAL
        {
            // SAFETY: context points to changed until CM_Unregister_Notification
            // returns, which waits for outstanding callbacks to finish.
            unsafe {
                (&*context.cast::<F>())();
            }
        }
        0
    }
    // GUID_DEVINTERFACE_COMPORT, notified once the COM interface is available.
    let guid = windows_sys::core::GUID {
        data1: 0x86e0d1e0,
        data2: 0x8089,
        data3: 0x11d0,
        data4: [0x9c, 0xe4, 0x08, 0x00, 0x3e, 0x30, 0x1f, 0x73],
    };
    let changed = Box::new(changed);
    let filter = CM_NOTIFY_FILTER {
        cbSize: std::mem::size_of::<CM_NOTIFY_FILTER>() as u32,
        FilterType: CM_NOTIFY_FILTER_TYPE_DEVICEINTERFACE,
        u: CM_NOTIFY_FILTER_0 {
            DeviceInterface: CM_NOTIFY_FILTER_0_0 { ClassGuid: guid },
        },
        ..Default::default()
    };
    let mut handle = std::ptr::null_mut();
    let result = unsafe {
        CM_Register_Notification(
            &filter,
            (&*changed as *const F).cast(),
            Some(notify::<F>),
            &mut handle,
        )
    };
    if result != CR_SUCCESS {
        let _ = ready.send(Err(io::Error::other(format!(
            "CM_Register_Notification: {result}"
        ))));
        return;
    }
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(250));
    }
    // SAFETY: unregister before dropping callback context.
    unsafe {
        CM_Unregister_Notification(handle);
    }
}
