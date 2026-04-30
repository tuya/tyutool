//! High-level flash operations: shake, erase, write, CRC check, reboot.
//!
//! These functions compose the frame/command/transport primitives into
//! the complete firmware-flashing workflow shared by BK7231N and T5.

use super::chip::ChipSpec;
use super::command::{self, build, parse};
use super::flash_table::{self, FlashParams};
use super::frame::ProtocolError;
use super::transport::{IoTransport, Transport};

/// Sector size constant (4 KiB) — internal.
const SECTOR_SIZE: u32 = 4096;

/// Sector size constant (4 KiB) — public for use by plugin wrappers.
pub const SECTOR_SIZE_PUB: u32 = SECTOR_SIZE;

// ─────────────────────────────────────────────────────────────────────────
// shake — handshake + baud-rate switch + post-handshake hook
// ─────────────────────────────────────────────────────────────────────────

/// Perform the full handshake sequence:
/// 1. Hardware reset (DTR/RTS) + LinkCheck loop (T5: retry reset on failure)
/// 2. Switch to target baud rate
/// 3. Chip-specific post-handshake (e.g. T5 reads ChipID)
pub fn shake<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    target_baud: u32,
    chip: &dyn ChipSpec,
    _is_t5: bool,
) -> Result<(), ProtocolError> {
    log::info!("Starting handshake sequence (target baud: {})", target_baud);
    transport.log(&format!("{}: starting handshake...", chip.name()));

    if chip.uses_extended_reset_sequence() {
        // T5: loop reset + link_check, matching Python get_bus() pattern
        let max_reset_retries: u32 = 10;
        let mut connected = false;
        for attempt in 0..max_reset_retries {
            transport.check_cancel()?;

            // Hardware reset
            transport.reset_into_download_mode_t5()?;
            std::thread::sleep(std::time::Duration::from_millis(4));
            transport.clear_rx();

            // LinkCheck with short timeout (Python uses 1ms timeout per attempt)
            match transport.handshake(chip.handshake_retries(), chip.handshake_interval_ms()) {
                Ok(()) => {
                    transport.log(&format!(
                        "{}: bus acquired (reset attempt {}/{})",
                        chip.name(),
                        attempt + 1,
                        max_reset_retries
                    ));
                    connected = true;
                    break;
                }
                Err(ProtocolError::Cancelled) => return Err(ProtocolError::Cancelled),
                Err(_) => {
                    // Reset and retry — this is normal for T5
                    continue;
                }
            }
        }
        if !connected {
            return Err(ProtocolError::Timeout {
                attempts: max_reset_retries,
            });
        }
    } else {
        // BK7231N/BK7231X/T2: Python shake() pattern (from bk7231n_flash.py):
        //
        // Step 1: reboot_cmd_tx() — 3× BKRegDoReboot (150ms apart) + DTR/RTS + one more BKRegDoReboot
        // Step 2: Continuous LinkCheck loop with 1ms timeout per attempt.
        //         Every 100 failed attempts → reboot_cmd_tx() again + baud toggle + "reboot\r\n"
        //         Up to 15 outer resets before giving up.
        //
        // KEY: The inner LinkCheck uses 1ms timeout (same as T5), and runs
        // continuously WITHOUT resetting the port between each attempt.
        // This is why 50 retries at 20ms each (~1s total) always fails —
        // the chip needs ~35–100 continuous 1ms link-checks after the reset pulse.

        let do_reboot = |transport: &mut Transport<'_, T>| -> Result<(), ProtocolError> {
            for _ in 0..3 {
                let tx =
                    super::frame::encode_standard(command::CMD_RESET, &build::bk_reg_do_reboot());
                let _ = transport.send_raw(&tx);
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            transport.reset_into_download_mode_bk()?;
            let tx = super::frame::encode_standard(command::CMD_RESET, &build::bk_reg_do_reboot());
            let _ = transport.send_raw(&tx);
            Ok(())
        };

        do_reboot(transport)?;

        let mut connected = false;
        let mut count_sec = 0u32;
        let mut outer = 0u32;

        loop {
            transport.check_cancel()?;

            // Single LinkCheck attempt with 1ms timeout (matching Python's 0.001s)
            match transport.handshake(1, 1) {
                Ok(()) => {
                    transport.log(&format!(
                        "{}: bus acquired (after ~{} link-checks, {} resets)",
                        chip.name(),
                        count_sec,
                        outer
                    ));
                    connected = true;
                    break;
                }
                Err(ProtocolError::Cancelled) => return Err(ProtocolError::Cancelled),
                Err(_) => {}
            }

            count_sec += 1;

            if count_sec > 100 {
                // Outer retry: reboot + baud toggle + "reboot\r\n" (Python behaviour)
                do_reboot(transport)?;
                count_sec = 0;
                outer += 1;

                if outer > 15 {
                    break;
                }

                // Send an extra BKRegDoReboot (Python does this after reboot_cmd_tx)
                let tx =
                    super::frame::encode_standard(command::CMD_RESET, &build::bk_reg_do_reboot());
                let _ = transport.send_raw(&tx);

                // Baud-rate toggle: Python toggles between 115200 and 921600
                // to wake up chips that may be stuck at a different baud rate
                let current_baud = chip.initial_baud();
                let toggle_baud = if current_baud == 115200 {
                    921600
                } else {
                    115200
                };
                let _ = transport.io.set_baud_rate(toggle_baud);

                // Send "reboot\r\n" text command (some bootloaders respond to this)
                let _ = transport.io.write_all(b"reboot\r\n");

                // Reset back to bootrom baud rate
                let _ = transport.io.set_baud_rate(current_baud);

                transport.log(&format!(
                    "{}: link check outer retry {}/15",
                    chip.name(),
                    outer
                ));
            }
        }

        if !connected {
            return Err(ProtocolError::Timeout {
                attempts: outer * 100,
            });
        }

        // Small flush after successful link-check (matches Python's Flush() call)
        std::thread::sleep(std::time::Duration::from_millis(10));
        transport.clear_rx();
    }

    // 3. Switch baud rate (if different from initial)
    if target_baud != chip.initial_baud() {
        transport.log(&format!(
            "{}: switching baud rate to {}",
            chip.name(),
            target_baud
        ));
        transport.switch_baud_rate(target_baud, chip.baud_switch_delay_ms())?;

        // Verify link at new baud rate
        transport.handshake(10, 50)?;
    }

    // 4. Chip-specific post-handshake
    if chip.needs_post_handshake() {
        super::chip::post_handshake_read_chip_id(transport, chip.name())?;
    }

    transport.log(&format!("{}: handshake complete", chip.name()));
    log::info!("Handshake complete, chip ready");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// get_flash_params — read Flash JEDEC MID and look up parameters
// ─────────────────────────────────────────────────────────────────────────

/// Read the flash chip's JEDEC MID and return its parameters.
///
/// Falls back to conservative defaults if the MID is not in the table.
///
/// For T5, the FlashGetMID uses extended frame format with register address
/// `0x9f` (JEDEC Read-ID command) in the payload.
/// For BK7231N, it uses standard frame with empty payload.
pub fn get_flash_params<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    chip: &dyn ChipSpec,
) -> Result<FlashParams, ProtocolError> {
    log::info!("Reading Flash MID...");
    let rx = if chip.use_extended_flash_mid() {
        // T5: extended frame with 0x9f register address
        let payload = build::flash_get_mid_ext(0x9f);
        transport.send_recv_extended(command::CMD_FLASH_GET_MID, &payload, 3000)?
    } else {
        // BK7231N: standard frame, empty payload
        transport.send_recv_standard(command::CMD_FLASH_GET_MID, &build::flash_get_mid(), 3000)?
    };

    let mid = if chip.use_extended_flash_mid() {
        // T5 extended response: payload has 4 bytes of MID data
        // The extended response format: cmd=0x0e, status=0x00, data=[mid0, mid1, mid2, mid3]
        // MID is in data bytes, skip first byte (status already checked)
        parse::flash_mid_from_extended(&rx.data)?
    } else {
        parse::flash_mid_from_standard(rx.status, &rx.data)?
    };
    transport.log(&format!("Flash MID: {mid:#08x}"));

    match flash_table::lookup(mid) {
        Some(params) => {
            transport.log(&format!(
                "Flash: {} ({}) — {} KiB",
                params.name,
                params.vendor,
                params.total_size / 1024
            ));
            log::debug!(
                "Flash MID: {:#08x}, chip: {} ({}), size: {} KiB",
                mid,
                params.name,
                params.vendor,
                params.total_size / 1024
            );
            Ok(*params)
        }
        None => {
            let fb = flash_table::fallback(mid);
            transport.log(&format!(
                "Flash MID {mid:#08x} not in table — using fallback ({} KiB)",
                fb.total_size / 1024
            ));
            log::debug!(
                "Flash MID: {:#08x} (unknown), using fallback {} KiB",
                mid,
                fb.total_size / 1024
            );
            Ok(fb)
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Write-protection: unprotect / protect
// ─────────────────────────────────────────────────────────────────────────

/// Disable flash write protection by clearing the BP bits in the Status Register.
pub fn unprotect_flash<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    params: &FlashParams,
    chip: &dyn ChipSpec,
) -> Result<(), ProtocolError> {
    log::info!("Unprotecting flash...");
    let wp = match &params.wp {
        Some(wp) => wp,
        None => {
            transport.log("no WP config — skipping unprotect");
            return Ok(());
        }
    };

    transport.log("disabling flash write protection...");

    if chip.use_extended_flash_ops() {
        // T5: uses extended frames for FlashReadSR / FlashWriteSR
        unprotect_flash_extended(transport, wp)?;
    } else if wp.sr_bytes == 1 {
        // BK7231N single SR
        let rx = transport.send_recv_standard(
            command::CMD_FLASH_READ_SR,
            &build::flash_read_sr(wp.read_sr_cmds[0]),
            3000,
        )?;
        let sr_val = parse::flash_sr_from_standard(&rx.data)?;
        let new_val = sr_val & !(wp.protect_mask as u8);
        transport.send_recv_standard(
            command::CMD_FLASH_WRITE_SR,
            &build::flash_write_sr(wp.write_sr_cmds[0], &[new_val]),
            3000,
        )?;
    } else {
        // BK7231N dual SR
        let rx1 = transport.send_recv_standard(
            command::CMD_FLASH_READ_SR,
            &build::flash_read_sr(wp.read_sr_cmds[0]),
            3000,
        )?;
        let sr1 = parse::flash_sr_from_standard(&rx1.data)?;

        let rx2 = transport.send_recv_standard(
            command::CMD_FLASH_READ_SR,
            &build::flash_read_sr(wp.read_sr_cmds[1]),
            3000,
        )?;
        let sr2 = parse::flash_sr_from_standard(&rx2.data)?;

        let sr16 = (sr1 as u16) | ((sr2 as u16) << 8);
        let new_sr16 = sr16 & !wp.protect_mask;
        let new_sr1 = (new_sr16 & 0xff) as u8;
        let new_sr2 = ((new_sr16 >> 8) & 0xff) as u8;

        if wp.write_sr_cmds.len() > 1 {
            transport.send_recv_standard(
                command::CMD_FLASH_WRITE_SR,
                &build::flash_write_sr(wp.write_sr_cmds[0], &[new_sr1]),
                3000,
            )?;
            transport.send_recv_standard(
                command::CMD_FLASH_WRITE_SR,
                &build::flash_write_sr(wp.write_sr_cmds[1], &[new_sr2]),
                3000,
            )?;
        } else {
            transport.send_recv_standard(
                command::CMD_FLASH_WRITE_SR,
                &build::flash_write_sr(wp.write_sr_cmds[0], &[new_sr1, new_sr2]),
                3000,
            )?;
        }
    }

    transport.log("write protection disabled");
    Ok(())
}

/// T5 unprotect using extended frames (matches Python _read_flash_status_reg_val / _write_flash_status_reg_val).
fn unprotect_flash_extended<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    wp: &super::flash_table::WriteProtectConfig,
) -> Result<(), ProtocolError> {
    // Read SR values using extended frames
    let sr_vals = read_sr_values_extended(transport, wp)?;

    // Build unprotect values
    let unprotect_reg_val: Vec<u8> = vec![0x00; sr_vals.len()];
    // mask layout matches Python: [0x7c, 0x40] for GD25WQ64E (protect_mask = 0x407c)
    let mask_lo = (wp.protect_mask & 0xff) as u8;
    let mask_hi = ((wp.protect_mask >> 8) & 0xff) as u8;
    let masks = if sr_vals.len() >= 2 {
        vec![mask_lo, mask_hi]
    } else {
        vec![mask_lo]
    };

    // Check if already unprotected
    let already_unprotected = sr_vals
        .iter()
        .zip(unprotect_reg_val.iter())
        .zip(masks.iter())
        .all(|((src, dest), mask)| (src & mask) == (dest & mask));

    if already_unprotected {
        transport.log("BP bits already clear, but writing SR to ensure flash is writable");
    }

    // Always write SR values to ensure flash is writable.
    // Even if BP bits look clear, other bits (like SRP0) may prevent
    // erase/write operations. Writing SR ensures the bootrom processes
    // the write-enable sequence internally.

    // Compute new values: unprotect bits cleared, other bits preserved
    let mut write_vals: Vec<u8> = Vec::new();
    for i in 0..sr_vals.len() {
        let new_val = unprotect_reg_val[i] | (sr_vals[i] & (masks[i] ^ 0xff));
        write_vals.push(new_val);
    }

    transport.log(&format!("SR before unprotect: {:02x?}", sr_vals));
    transport.log(&format!("SR write values: {:02x?}", write_vals));

    // Write back using extended frames with separate commands
    write_sr_values_extended(transport, wp, &write_vals)?;

    // Verify: re-read SR and check unprotect took effect
    let sr_after = read_sr_values_extended(transport, wp)?;
    transport.log(&format!("SR after unprotect: {:02x?}", sr_after));

    let verified = sr_after
        .iter()
        .zip(unprotect_reg_val.iter())
        .zip(masks.iter())
        .all(|((src, dest), mask)| (src & mask) == (dest & mask));

    if !verified {
        return Err(ProtocolError::Protocol(format!(
            "unprotect flash failed: SR after write = {:02x?}",
            sr_after
        )));
    }

    Ok(())
}

/// Read flash Status Register values using extended frames.
fn read_sr_values_extended<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    wp: &super::flash_table::WriteProtectConfig,
) -> Result<Vec<u8>, ProtocolError> {
    let mut sr_vals: Vec<u8> = Vec::new();
    for &read_cmd in wp.read_sr_cmds {
        let rx = transport.send_recv_extended(
            command::CMD_FLASH_READ_SR,
            &build::flash_read_sr(read_cmd),
            3000,
        )?;
        let sr_val = parse::flash_sr_from_extended(&rx.data)?;
        sr_vals.push(sr_val);
    }
    Ok(sr_vals)
}

/// Write flash Status Register values using extended frames.
fn write_sr_values_extended<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    wp: &super::flash_table::WriteProtectConfig,
    write_vals: &[u8],
) -> Result<(), ProtocolError> {
    if wp.write_sr_cmds.len() > 1 {
        for (i, &write_cmd) in wp.write_sr_cmds.iter().enumerate() {
            if i < write_vals.len() {
                transport.send_recv_extended(
                    command::CMD_FLASH_WRITE_SR,
                    &build::flash_write_sr(write_cmd, &[write_vals[i]]),
                    3000,
                )?;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    } else {
        transport.send_recv_extended(
            command::CMD_FLASH_WRITE_SR,
            &build::flash_write_sr(wp.write_sr_cmds[0], write_vals),
            3000,
        )?;
    }
    Ok(())
}

/// Re-enable flash write protection.
pub fn protect_flash<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    params: &FlashParams,
    chip: &dyn ChipSpec,
) -> Result<(), ProtocolError> {
    let wp = match &params.wp {
        Some(wp) => wp,
        None => return Ok(()),
    };

    transport.log("re-enabling flash write protection...");

    if chip.use_extended_flash_ops() {
        // T5: use extended frames for protect
        protect_flash_extended(transport, wp)?;
    } else if wp.sr_bytes == 1 {
        let val = (wp.protect_value & 0xff) as u8;
        transport.send_recv_standard(
            command::CMD_FLASH_WRITE_SR,
            &build::flash_write_sr(wp.write_sr_cmds[0], &[val]),
            3000,
        )?;
    } else {
        let val1 = (wp.protect_value & 0xff) as u8;
        let val2 = ((wp.protect_value >> 8) & 0xff) as u8;
        if wp.write_sr_cmds.len() > 1 {
            transport.send_recv_standard(
                command::CMD_FLASH_WRITE_SR,
                &build::flash_write_sr(wp.write_sr_cmds[0], &[val1]),
                3000,
            )?;
            transport.send_recv_standard(
                command::CMD_FLASH_WRITE_SR,
                &build::flash_write_sr(wp.write_sr_cmds[1], &[val2]),
                3000,
            )?;
        } else {
            transport.send_recv_standard(
                command::CMD_FLASH_WRITE_SR,
                &build::flash_write_sr(wp.write_sr_cmds[0], &[val1, val2]),
                3000,
            )?;
        }
    }

    transport.log("write protection re-enabled");
    Ok(())
}

/// T5 protect using extended frames (matches Python pattern).
fn protect_flash_extended<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    wp: &super::flash_table::WriteProtectConfig,
) -> Result<(), ProtocolError> {
    // Read current SR values
    let mut sr_vals: Vec<u8> = Vec::new();
    for &read_cmd in wp.read_sr_cmds {
        let rx = transport.send_recv_extended(
            command::CMD_FLASH_READ_SR,
            &build::flash_read_sr(read_cmd),
            3000,
        )?;
        let sr_val = parse::flash_sr_from_extended(&rx.data)?;
        sr_vals.push(sr_val);
    }

    // Build protect values and masks
    let mask_lo = (wp.protect_mask & 0xff) as u8;
    let mask_hi = ((wp.protect_mask >> 8) & 0xff) as u8;
    let masks = if sr_vals.len() >= 2 {
        vec![mask_lo, mask_hi]
    } else {
        vec![mask_lo]
    };

    let protect_lo = (wp.protect_value & 0xff) as u8;
    let protect_hi = ((wp.protect_value >> 8) & 0xff) as u8;
    let protect_vals = if sr_vals.len() >= 2 {
        vec![protect_lo, protect_hi]
    } else {
        vec![protect_lo]
    };

    // Check if already protected
    let already_protected = sr_vals
        .iter()
        .zip(protect_vals.iter())
        .zip(masks.iter())
        .all(|((src, dest), mask)| (src & mask) == (dest & mask));

    if already_protected {
        return Ok(());
    }

    // Compute new values
    let mut write_vals: Vec<u8> = Vec::new();
    for i in 0..sr_vals.len() {
        let new_val = protect_vals[i] | (sr_vals[i] & (masks[i] ^ 0xff));
        write_vals.push(new_val);
    }

    // Write back
    if wp.write_sr_cmds.len() > 1 {
        for (i, &write_cmd) in wp.write_sr_cmds.iter().enumerate() {
            if i < write_vals.len() {
                transport.send_recv_extended(
                    command::CMD_FLASH_WRITE_SR,
                    &build::flash_write_sr(write_cmd, &[write_vals[i]]),
                    3000,
                )?;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    } else {
        transport.send_recv_extended(
            command::CMD_FLASH_WRITE_SR,
            &build::flash_write_sr(wp.write_sr_cmds[0], &write_vals),
            3000,
        )?;
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// erase — erase a flash address range
// ─────────────────────────────────────────────────────────────────────────

/// Erase the flash region `[start_addr, end_addr)`.
///
/// Uses 64 KiB block erase where possible, falling back to 4 KiB sector
/// erase for unaligned boundaries.
///
/// `progress_cb(erased_bytes, total_bytes)` is called after each erase op.
pub fn erase<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    _params: &FlashParams,
    chip: &dyn ChipSpec,
    start_addr: u32,
    end_addr: u32,
    progress_cb: &dyn Fn(u32, u32),
) -> Result<(), ProtocolError> {
    if start_addr >= end_addr {
        return Ok(());
    }

    let total = end_addr - start_addr;
    let mut erased: u32 = 0;
    let mut addr = start_addr;

    let block_size: u32 = 64 * 1024;

    log::info!(
        "Erasing flash: {:#010x}..{:#010x} ({} KiB)",
        start_addr,
        end_addr,
        total / 1024
    );
    transport.log(&format!(
        "erasing {:#010x}..{:#010x} ({} KiB)",
        start_addr,
        end_addr,
        total / 1024
    ));

    while addr < end_addr {
        transport.check_cancel()?;

        let remaining = end_addr - addr;

        // Use 64K block erase when the chip supports it, the address is aligned,
        // and there's enough remaining data to fill a full block.
        // All current chips (BK7231N, T5, T2) support 64K block erase.
        // CRC verification after erase catches any failure, with 4K fallback.
        let use_block_erase =
            chip.use_block_erase_64k() && addr % block_size == 0 && remaining >= block_size;

        if use_block_erase {
            // 64K block erase
            log::debug!("Erasing 64K block at {:#010x}", addr);
            let erase_cmd = if chip.use_extended_erase() {
                command::ERASE_CMD_64K_EXT
            } else {
                command::ERASE_CMD_64K
            };
            let payload = build::flash_erase(erase_cmd, addr);
            let erase_rx = if chip.use_extended_flash_ops() {
                transport.send_recv_extended(command::CMD_SET_BAUD_RATE, &payload, 10_000)?
            } else {
                transport.send_recv_standard(command::CMD_SET_BAUD_RATE, &payload, 10_000)?
            };
            // Check erase response status
            if erase_rx.status != 0x00 {
                transport.log(&format!(
                    "64K erase at {addr:#010x} FAILED: status={:#04x}",
                    erase_rx.status
                ));
                return Err(ProtocolError::DeviceError(erase_rx.status));
            }

            // Verify erase: CRC check the first 4K of the erased block
            let verify_end = addr + SECTOR_SIZE - 1;
            let crc_payload = build::check_crc(addr, verify_end);
            let crc_rx =
                transport.send_recv_standard(command::CMD_CHECK_CRC, &crc_payload, 3000)?;
            let device_crc = parse::crc32_from_standard(crc_rx.status, &crc_rx.data)?;
            let blank_crc = crc32_ver2(&[0xFFu8; SECTOR_SIZE as usize]);
            if device_crc != blank_crc {
                transport.log(&format!(
                    "64K erase at {addr:#010x} VERIFY FAILED: CRC={device_crc:#010x} expected_blank={blank_crc:#010x}"
                ));
                // Try erase again with 4K sectors as fallback
                transport.log(&format!(
                    "falling back to 4K sector erase for block at {addr:#010x}"
                ));
                for sector_offset in (0..block_size).step_by(SECTOR_SIZE as usize) {
                    let sector_addr = addr + sector_offset;
                    let erase_4k_cmd = if chip.use_extended_erase() {
                        command::ERASE_CMD_4K_EXT
                    } else {
                        command::ERASE_CMD_4K
                    };
                    let payload_4k = build::flash_erase(erase_4k_cmd, sector_addr);
                    if chip.use_extended_flash_ops() {
                        transport.send_recv_extended(
                            command::CMD_SET_BAUD_RATE,
                            &payload_4k,
                            5_000,
                        )?;
                    } else {
                        transport.send_recv_standard(
                            command::CMD_SET_BAUD_RATE,
                            &payload_4k,
                            5_000,
                        )?;
                    }
                }
            }

            addr += block_size;
            erased += block_size;
        } else {
            // 4K sector erase
            log::debug!("Erasing 4K sector at {:#010x}", addr);
            let erase_cmd = if chip.use_extended_erase() {
                command::ERASE_CMD_4K_EXT
            } else {
                command::ERASE_CMD_4K
            };
            let payload = build::flash_erase(erase_cmd, addr);
            let erase_rx = if chip.use_extended_flash_ops() {
                transport.send_recv_extended(command::CMD_SET_BAUD_RATE, &payload, 5_000)?
            } else {
                transport.send_recv_standard(command::CMD_SET_BAUD_RATE, &payload, 5_000)?
            };
            // Check erase response status
            if erase_rx.status != 0x00 {
                return Err(ProtocolError::DeviceError(erase_rx.status));
            }
            addr += SECTOR_SIZE;
            erased += SECTOR_SIZE;
        }

        progress_cb(erased.min(total), total);
    }

    transport.log("erase complete");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// write — write firmware data to flash
// ─────────────────────────────────────────────────────────────────────────

/// Write `firmware` bytes to flash starting at `base_addr`.
///
/// Writes in 4 KiB chunks. If the chip supports per-sector CRC (`T5Spec`),
/// each sector is verified immediately after write.
///
/// `progress_cb(written_bytes, total_bytes)` is called after each chunk.
pub fn write<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    _params: &FlashParams,
    chip: &dyn ChipSpec,
    firmware: &[u8],
    base_addr: u32,
    progress_cb: &dyn Fn(u32, u32),
) -> Result<(), ProtocolError> {
    let total = firmware.len() as u32;
    let mut written: u32 = 0;

    /// Maximum retries for write+CRC per sector before giving up.
    const MAX_SECTOR_RETRIES: u32 = 5;

    log::info!("Writing flash: {} KiB to {:#010x}", total / 1024, base_addr);
    transport.log(&format!(
        "writing {} KiB to {:#010x}...",
        total / 1024,
        base_addr
    ));

    for chunk in firmware.chunks(SECTOR_SIZE as usize) {
        transport.check_cancel()?;

        let addr = base_addr + written;
        let extended = chip.use_extended_frame(addr);

        log::debug!("Writing sector at {:#010x} ({} bytes)", addr, chunk.len());

        // Pad chunk to 4K if it's the last (short) chunk
        let mut sector = [0xFFu8; SECTOR_SIZE as usize];
        sector[..chunk.len()].copy_from_slice(chunk);

        // Skip blank sectors (T5 optimisation)
        if chip.skip_blank_sectors() && is_blank(&sector) {
            written += chunk.len() as u32;
            progress_cb(written, total);
            continue;
        }

        // Write sector with CRC verification and retry on mismatch
        let mut sector_ok = false;
        for attempt in 0..MAX_SECTOR_RETRIES {
            transport.check_cancel()?;

            if attempt > 0 {
                transport.log(&format!(
                    "retry write at {addr:#010x} (attempt {}/{})",
                    attempt + 1,
                    MAX_SECTOR_RETRIES
                ));
                // Clear buffers for clean retry
                transport.clear_rx();
                // Small delay before retry
                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            // Build and send FlashWrite4K
            let payload = build::flash_write_4k(addr, &sector);
            let write_result = transport.send_recv_retry(
                command::CMD_FLASH_WRITE_4K,
                &payload,
                extended,
                5_000, // write can take a moment
                4,     // up to 4 retries for frame-level send/recv
            );

            let write_rx = match write_result {
                Ok(rx) => rx,
                Err(e) => {
                    transport.log(&format!(
                        "write send/recv error at {addr:#010x} (attempt {}): {e}",
                        attempt + 1
                    ));
                    continue; // retry
                }
            };

            // Ensure the OS serial TX buffer is fully drained before the
            // next sector command goes out. On macOS CDC ACM (CH34x etc.)
            // this avoids residual TX bytes overlapping with the next
            // FlashWrite4K and causing spurious CRC failures.
            if let Err(e) = transport.io.flush() {
                log::debug!("flush after write at {addr:#010x}: {e}");
            }

            // Check write response status (extended frame: status must be 0x00)
            if write_rx.status != 0x00 {
                transport.log(&format!(
                    "write at {addr:#010x} returned status {:#04x} (attempt {})",
                    write_rx.status,
                    attempt + 1
                ));
                continue; // retry
            }

            // Per-sector CRC verification (T5)
            if chip.has_per_sector_crc() {
                // Clear RX buffer to avoid stale data from write response
                transport.clear_rx();

                let end_addr_inclusive = addr + SECTOR_SIZE - 1;
                let crc_payload = build::check_crc(addr, end_addr_inclusive);
                let crc_result =
                    transport.send_recv_standard(command::CMD_CHECK_CRC, &crc_payload, 3000);

                let rx = match crc_result {
                    Ok(rx) => rx,
                    Err(e) => {
                        transport.log(&format!(
                            "CRC check error at {addr:#010x} (attempt {}): {e}",
                            attempt + 1
                        ));
                        continue;
                    }
                };

                let device_crc = match parse::crc32_from_standard(rx.status, &rx.data) {
                    Ok(crc) => crc,
                    Err(e) => {
                        transport.log(&format!(
                            "CRC parse error at {addr:#010x} (attempt {}): {e}",
                            attempt + 1
                        ));
                        continue;
                    }
                };
                // T5 bootrom uses crc32_ver2 (no final XOR)
                let local_crc = crc32_ver2(&sector);
                if device_crc != local_crc {
                    transport.log(&format!(
                        "CRC mismatch at {addr:#010x} (attempt {}): local={local_crc:#010x} device={device_crc:#010x}",
                        attempt + 1
                    ));
                    continue; // retry write + CRC
                }
            }

            sector_ok = true;
            break;
        }

        if !sector_ok {
            transport.log(&format!(
                "write at {addr:#010x} failed after {MAX_SECTOR_RETRIES} attempts"
            ));
            return Err(ProtocolError::Protocol(format!(
                "sector write+CRC failed at {addr:#010x} after {MAX_SECTOR_RETRIES} retries"
            )));
        }

        written += chunk.len() as u32;
        progress_cb(written, total);
    }

    transport.log("write complete");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// crc_check — whole-image CRC verification (BK7231N)
// ─────────────────────────────────────────────────────────────────────────

/// Verify the CRC-32 of the written firmware region against the device.
///
/// Used by BK7231N (which doesn't do per-sector CRC during write).
pub fn crc_check<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    base_addr: u32,
    len: u32,
    expected_crc: u32,
) -> Result<(), ProtocolError> {
    log::info!("CRC check: {:#010x}..{:#010x}", base_addr, base_addr + len);
    transport.log(&format!(
        "verifying CRC @ {:#010x}..{:#010x}",
        base_addr,
        base_addr + len
    ));

    let end_addr_inclusive = base_addr + len - 1;
    let payload = build::check_crc(base_addr, end_addr_inclusive);
    let rx = transport.send_recv_standard(command::CMD_CHECK_CRC, &payload, 10_000)?;
    let device_crc = parse::crc32_from_standard(rx.status, &rx.data)?;

    if device_crc != expected_crc {
        log::error!(
            "CRC mismatch: expected={:#010x}, got={:#010x}",
            expected_crc,
            device_crc
        );
        return Err(ProtocolError::CrcMismatch {
            expected: expected_crc,
            got: device_crc,
        });
    }

    transport.log(&format!("CRC OK: {expected_crc:#010x}"));
    log::info!("CRC check passed");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// read — read flash contents to a buffer
// ─────────────────────────────────────────────────────────────────────────

/// Read `length` bytes from flash starting at `start_addr`.
///
/// Reads in 4 KiB sectors using `FlashRead4K` (always extended frame).
/// For T5, each sector is additionally verified via per-sector CRC.
///
/// Returns the read data trimmed to exactly `length` bytes.
///
/// `progress_cb(read_bytes, total_bytes)` is called after each sector.
pub fn read<T: IoTransport>(
    transport: &mut Transport<'_, T>,
    _params: &FlashParams,
    chip: &dyn ChipSpec,
    start_addr: u32,
    length: u32,
    progress_cb: &dyn Fn(u32, u32),
) -> Result<Vec<u8>, ProtocolError> {
    if length == 0 {
        return Ok(Vec::new());
    }

    // Align down to 4K boundary
    let read_addr = start_addr & !(SECTOR_SIZE - 1);
    let offset = start_addr - read_addr;
    // Total bytes to read (aligned up to cover the full range)
    let total_aligned = ((offset + length) + SECTOR_SIZE - 1) & !(SECTOR_SIZE - 1);

    let mut buf: Vec<u8> = Vec::with_capacity(total_aligned as usize);
    let mut read_so_far: u32 = 0;

    /// Maximum consecutive read failures before giving up.
    const MAX_CONSECUTIVE_FAILURES: u32 = 10;
    /// Maximum retries per sector for T5 CRC verification.
    const MAX_SECTOR_RETRIES: u32 = 5;

    transport.log(&format!(
        "reading {} KiB from {:#010x}...",
        length / 1024,
        start_addr
    ));

    while read_so_far < total_aligned {
        transport.check_cancel()?;

        let addr = read_addr + read_so_far;

        if chip.has_per_sector_crc() {
            // T5: read + CRC verify (matching Python read_and_check_sector)
            let mut sector_ok = false;
            for attempt in 0..MAX_SECTOR_RETRIES {
                transport.check_cancel()?;

                let payload = build::flash_read_4k(addr);
                let rx_result =
                    transport.send_recv_extended(command::CMD_FLASH_READ_4K, &payload, 5_000);

                let rx = match rx_result {
                    Ok(rx) => rx,
                    Err(e) => {
                        transport.log(&format!(
                            "read error at {addr:#010x} (attempt {}/{}): {e}",
                            attempt + 1,
                            MAX_SECTOR_RETRIES
                        ));
                        continue;
                    }
                };

                let sector_data = match parse::flash_read_4k_data(&rx.data) {
                    Ok(d) => d,
                    Err(e) => {
                        transport.log(&format!(
                            "read parse error at {addr:#010x} (attempt {}/{}): {e}",
                            attempt + 1,
                            MAX_SECTOR_RETRIES
                        ));
                        continue;
                    }
                };

                if sector_data.len() < SECTOR_SIZE as usize {
                    transport.log(&format!(
                        "read sector short at {addr:#010x}: {} bytes (attempt {}/{})",
                        sector_data.len(),
                        attempt + 1,
                        MAX_SECTOR_RETRIES
                    ));
                    continue;
                }

                // CRC verification (T5 uses crc32_ver2)
                let local_crc = crc32_ver2(&sector_data[..SECTOR_SIZE as usize]);
                let end_addr_inclusive = addr + SECTOR_SIZE - 1;
                let crc_payload = build::check_crc(addr, end_addr_inclusive);
                let crc_result =
                    transport.send_recv_standard(command::CMD_CHECK_CRC, &crc_payload, 3000);

                match crc_result {
                    Ok(crc_rx) => match parse::crc32_from_standard(crc_rx.status, &crc_rx.data) {
                        Ok(device_crc) => {
                            if device_crc == local_crc {
                                buf.extend_from_slice(&sector_data[..SECTOR_SIZE as usize]);
                                sector_ok = true;
                                break;
                            } else {
                                transport.log(&format!(
                                        "read CRC mismatch at {addr:#010x} (attempt {}/{}): local={local_crc:#010x} device={device_crc:#010x}",
                                        attempt + 1,
                                        MAX_SECTOR_RETRIES
                                    ));
                            }
                        }
                        Err(e) => {
                            transport.log(&format!(
                                "read CRC parse error at {addr:#010x} (attempt {}/{}): {e}",
                                attempt + 1,
                                MAX_SECTOR_RETRIES
                            ));
                        }
                    },
                    Err(e) => {
                        transport.log(&format!(
                            "read CRC check error at {addr:#010x} (attempt {}/{}): {e}",
                            attempt + 1,
                            MAX_SECTOR_RETRIES
                        ));
                    }
                }
            }

            if !sector_ok {
                return Err(ProtocolError::Protocol(format!(
                    "read failed at {addr:#010x} after {MAX_SECTOR_RETRIES} attempts"
                )));
            }
        } else {
            // BK7231N: read without per-sector CRC, retry on failure
            let mut consecutive_failures: u32 = 0;
            loop {
                transport.check_cancel()?;

                let payload = build::flash_read_4k(addr);
                // FlashRead4K always uses extended frame for both BK7231N and T5
                let rx_result =
                    transport.send_recv_extended(command::CMD_FLASH_READ_4K, &payload, 5_000);

                let rx = match rx_result {
                    Ok(rx) => rx,
                    Err(e) => {
                        consecutive_failures += 1;
                        transport.log(&format!(
                            "read error at {addr:#010x} ({consecutive_failures}/{MAX_CONSECUTIVE_FAILURES}): {e}"
                        ));
                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            transport
                                .log("read failure too many times. You can try other baudrate.");
                            return Err(ProtocolError::Protocol(format!(
                                "read failed at {addr:#010x} after {MAX_CONSECUTIVE_FAILURES} consecutive failures"
                            )));
                        }
                        continue;
                    }
                };

                let sector_data = match parse::flash_read_4k_data(&rx.data) {
                    Ok(d) => d,
                    Err(e) => {
                        consecutive_failures += 1;
                        transport.log(&format!(
                            "read parse error at {addr:#010x} ({consecutive_failures}/{MAX_CONSECUTIVE_FAILURES}): {e}"
                        ));
                        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                            return Err(ProtocolError::Protocol(format!(
                                "read failed at {addr:#010x} after {MAX_CONSECUTIVE_FAILURES} consecutive failures"
                            )));
                        }
                        continue;
                    }
                };

                if sector_data.len() < SECTOR_SIZE as usize {
                    consecutive_failures += 1;
                    transport.log(&format!(
                        "read sector not enough 4K: [{}] ({consecutive_failures}/{MAX_CONSECUTIVE_FAILURES})",
                        sector_data.len()
                    ));
                    if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
                        transport.log("read failure too many times. You can try other baudrate.");
                        return Err(ProtocolError::Protocol(format!(
                            "read failed at {addr:#010x} after {MAX_CONSECUTIVE_FAILURES} consecutive failures"
                        )));
                    }
                    continue;
                }

                buf.extend_from_slice(&sector_data[..SECTOR_SIZE as usize]);
                break;
            }
        }

        read_so_far += SECTOR_SIZE;
        progress_cb(read_so_far.min(total_aligned), total_aligned);
    }

    // Trim to exact requested range
    let result = buf[offset as usize..(offset + length) as usize].to_vec();

    transport.log(&format!("read complete ({} bytes)", result.len()));
    Ok(result)
}

// ─────────────────────────────────────────────────────────────────────────
// reboot
// ─────────────────────────────────────────────────────────────────────────

/// Send reboot command(s) to make the device boot from flash.
///
/// BK7231N: send CMD 0x0e with payload 0xa5 (reboot marker), 3 times.
pub fn reboot<T: IoTransport>(transport: &mut Transport<'_, T>) -> Result<(), ProtocolError> {
    log::info!("Rebooting device...");
    transport.log("rebooting device...");

    for _ in 0..3 {
        let payload = build::reboot();
        let tx = super::frame::encode_standard(command::CMD_FLASH_GET_MID, &payload);
        let _ = transport.send_raw(&tx);
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    transport.log("reboot command sent");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────

/// Compute CRC-32 (IEEE 802.3) of a buffer.
///
/// Uses `crc32fast` for SIMD-accelerated computation.
/// Compatible with Python `binascii.crc32`.
pub fn crc32_buf(data: &[u8]) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

/// Compute CRC-32 matching Python `crc32_ver2(0xFFFFFFFF, data)`.
///
/// This is the same algorithm as IEEE CRC-32 but **without** the final XOR
/// with `0xFFFFFFFF`. The T5 bootrom uses this variant for per-sector CRC
/// verification during writes.
pub fn crc32_ver2(data: &[u8]) -> u32 {
    // crc32fast computes: init=0xFFFFFFFF, then final XOR 0xFFFFFFFF
    // Python crc32_ver2: init=0xFFFFFFFF, no final XOR
    // So: crc32_ver2 = crc32fast ^ 0xFFFFFFFF
    crc32_buf(data) ^ 0xFFFF_FFFF
}

/// Check if a 4 KiB sector is entirely blank (0xFF).
fn is_blank(sector: &[u8]) -> bool {
    sector.iter().all(|&b| b == 0xFF)
}

/// Parse a hex address string (e.g. `"0x10000"`) to `u32`.
pub fn parse_hex_addr(s: Option<&str>) -> Result<u32, ProtocolError> {
    let s = s.ok_or_else(|| ProtocolError::Protocol("missing address".into()))?;
    let s = s.trim().trim_start_matches("0x").trim_start_matches("0X");
    u32::from_str_radix(s, 16)
        .map_err(|_| ProtocolError::Protocol(format!("invalid hex address: {s}")))
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_python() {
        // Python: binascii.crc32(b'\x00\x01\x02\x03') & 0xFFFFFFFF = 0x8BB98613
        let crc = crc32_buf(&[0x00, 0x01, 0x02, 0x03]);
        assert_eq!(crc, 0x8BB9_8613);
    }

    #[test]
    fn crc32_empty() {
        let crc = crc32_buf(&[]);
        assert_eq!(crc, 0x0000_0000);
    }

    #[test]
    fn is_blank_all_ff() {
        let sector = [0xFFu8; 4096];
        assert!(is_blank(&sector));
    }

    #[test]
    fn is_blank_not_blank() {
        let mut sector = [0xFFu8; 4096];
        sector[2048] = 0x00;
        assert!(!is_blank(&sector));
    }

    #[test]
    fn parse_hex_addr_works() {
        assert_eq!(parse_hex_addr(Some("0x10000")).unwrap(), 0x10000);
        assert_eq!(parse_hex_addr(Some("0X0")).unwrap(), 0);
        assert_eq!(parse_hex_addr(Some("1000")).unwrap(), 0x1000);
        assert!(parse_hex_addr(None).is_err());
        assert!(parse_hex_addr(Some("xyz")).is_err());
    }
}
