//! Chip-specific behaviour abstraction.
//!
//! BK7231N and T5AI share the same UART protocol but differ in:
//! - reset sequence timing
//! - post-handshake steps (T5AI reads ChipID)
//! - use of extended frames for large flash
//! - per-sector CRC after write (T5AI) vs. whole-image CRC after write (BK7231N)
//! - skipping blank (0xFF) sectors (T5AI optimisation)
//! - default baud-rate delay parameter

use super::frame::ProtocolError;
use super::transport::{IoTransport, Transport};

/// Trait capturing the behavioural differences between Beken chip families.
///
/// All methods are dyn-compatible (no generics). The `post_handshake`
/// step is handled separately via [`t5ai_post_handshake`] since it needs
/// a generic `Transport<T>`.
pub trait ChipSpec: Send + Sync {
    /// Human-readable chip name (for log messages).
    fn name(&self) -> &'static str;

    /// Initial baud rate for establishing the serial link (before switching).
    fn initial_baud(&self) -> u32 {
        115200
    }

    /// Maximum number of `LinkCheck` attempts before giving up.
    fn handshake_retries(&self) -> u32 {
        50
    }

    /// Timeout in ms for each handshake `LinkCheck` attempt response.
    /// Python T5AI uses 1ms; BK7231N uses 20ms.
    fn handshake_interval_ms(&self) -> u64 {
        20
    }

    /// Delay byte sent with `SetBaudRate` command (ms for the device to switch).
    fn baud_switch_delay_ms(&self) -> u8 {
        100
    }

    /// Whether this chip needs a post-handshake step (e.g. T5AI/T2 GetChipID).
    fn needs_post_handshake(&self) -> bool {
        false
    }

    /// Whether this chip uses the T5AI-style reset-retry handshake sequence
    /// (reset loop + extended link-check), as opposed to BK7231N-style
    /// (3× BKRegDoReboot + single DTR/RTS reset).
    fn uses_extended_reset_sequence(&self) -> bool {
        false
    }

    /// Whether to use extended (long) frame encoding for flash write at `addr`.
    fn use_extended_frame(&self, _addr: u32) -> bool {
        false
    }

    /// Whether the chip does per-sector CRC verification during write
    /// (eliminating the need for a separate whole-image CRC check).
    fn has_per_sector_crc(&self) -> bool {
        false
    }

    /// Whether to skip writing sectors that are entirely `0xFF`.
    fn skip_blank_sectors(&self) -> bool {
        false
    }

    /// Whether to use extended erase commands (`0x21`/`0xdc`) for large flash.
    ///
    /// Only needed for flash ≥256 MiB (4-byte addressing). All current chips
    /// use flash ≤8 MiB so they use standard commands (`0x20`/`0xd8`).
    /// Confirmed by vendor tool and Python reference:
    /// `flash_size >= 256*1024*1024` → extended, else standard.
    fn use_extended_erase(&self) -> bool {
        false
    }

    /// Whether this chip can safely use 64K block erase (`0xd8`/`0xdc`).
    ///
    /// All current Beken chips (BK7231N, T5AI, T2) support 64K block erase.
    /// The Python reference and vendor tool confirm T5AI uses 64K erase
    /// (command `0xd8` for flash <256 MiB, `0xdc` for flash ≥256 MiB).
    /// The erase path includes CRC verification with automatic 4K fallback
    /// for safety.
    fn use_block_erase_64k(&self) -> bool {
        true
    }

    /// Whether FlashGetMID uses extended frame format (T5AI).
    fn use_extended_flash_mid(&self) -> bool {
        false
    }

    /// Whether flash operations (FlashReadSR, FlashWriteSR, FlashErase, etc.)
    /// use extended frame format. T5AI uses extended frames for all flash
    /// operations; BK7231N uses standard frames.
    fn use_extended_flash_ops(&self) -> bool {
        false
    }
}

// ─────────────────────────────────────────────────────────────────────────
// BK7231N
// ─────────────────────────────────────────────────────────────────────────

/// BK7231N chip specification.
///
/// Standard Beken protocol: standard frames for LinkCheck/SetBaudRate/CheckCRC/Reboot,
/// but **extended frames** for all flash operations (FlashGetMID, FlashRead4K,
/// FlashWrite4K, FlashErase, FlashReadSR, FlashWriteSR).
/// Post-write whole-image CRC check, reset via 3× `BKRegDoReboot` + DTR/RTS pulse.
pub struct Bk7231nSpec;

impl ChipSpec for Bk7231nSpec {
    fn name(&self) -> &'static str {
        "BK7231N"
    }

    fn baud_switch_delay_ms(&self) -> u8 {
        100
    }

    fn use_extended_frame(&self, _addr: u32) -> bool {
        true
    }

    fn use_extended_flash_mid(&self) -> bool {
        true
    }

    fn use_extended_flash_ops(&self) -> bool {
        true
    }

    fn use_block_erase_64k(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────
// T5AI
// ─────────────────────────────────────────────────────────────────────────

/// T5AI chip registers for reading ChipID (tried in order).
pub const T5AI_CHIP_ID_REGS: &[u32] = &[0x4401_0004, 0x0080_0000, 0x3401_0004];

/// T5AI chip specification.
///
/// Differences from BK7231N:
/// - Post-handshake reads ChipID via `ReadReg`.
/// - Skips blank sectors during write.
/// - Per-sector CRC verification (no separate whole-image CRC step).
/// - Uses extended erase commands for flash >2 MiB.
/// - Lower baud-switch delay (20ms vs 100ms).
pub struct T5AISpec;

impl ChipSpec for T5AISpec {
    fn name(&self) -> &'static str {
        "T5AI"
    }

    fn handshake_interval_ms(&self) -> u64 {
        // Python T5AI uses 1ms timeout per LinkCheck attempt
        1
    }

    fn baud_switch_delay_ms(&self) -> u8 {
        20
    }

    fn needs_post_handshake(&self) -> bool {
        true
    }

    fn uses_extended_reset_sequence(&self) -> bool {
        true
    }

    fn use_extended_frame(&self, _addr: u32) -> bool {
        // T5AI ALWAYS uses extended frames for FlashWrite4K,
        // regardless of address. Extended-address commands (0xe7 etc.)
        // are only for flash >= 256 MiB.
        true
    }

    fn has_per_sector_crc(&self) -> bool {
        true
    }

    fn skip_blank_sectors(&self) -> bool {
        true
    }

    fn use_extended_erase(&self) -> bool {
        // T5AI uses standard erase commands (0x20/0xd8) for flash <256 MiB.
        // Extended commands (0x21/0xdc) are only for flash ≥256 MiB.
        // Confirmed by vendor tool pcapng: all erases use 0xd8 and 0x20.
        // Python reference: flash_size >= 256*1024*1024 → extended, else standard.
        false
    }

    fn use_extended_flash_mid(&self) -> bool {
        true
    }

    fn use_extended_flash_ops(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────
// T1
// ─────────────────────────────────────────────────────────────────────────

/// T1 chip specification — same protocol behaviour as [`T5AISpec`].
pub struct T1Spec;

impl ChipSpec for T1Spec {
    fn name(&self) -> &'static str {
        "T1"
    }

    fn handshake_interval_ms(&self) -> u64 {
        1
    }

    fn baud_switch_delay_ms(&self) -> u8 {
        20
    }

    fn needs_post_handshake(&self) -> bool {
        true
    }

    fn uses_extended_reset_sequence(&self) -> bool {
        true
    }

    fn use_extended_frame(&self, _addr: u32) -> bool {
        true
    }

    fn has_per_sector_crc(&self) -> bool {
        true
    }

    fn skip_blank_sectors(&self) -> bool {
        true
    }

    fn use_extended_erase(&self) -> bool {
        false
    }

    fn use_extended_flash_mid(&self) -> bool {
        true
    }

    fn use_extended_flash_ops(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────
// T2
// ─────────────────────────────────────────────────────────────────────────

/// T2 chip registers for reading ChipID (tried in order).
pub const T2_CHIP_ID_REGS: &[u32] = &[0x4401_0004, 0x0080_0000, 0x3401_0004];

/// T2 chip specification.
///
/// T2 uses the same Beken UART protocol as BK7231N (extended frames for flash ops,
/// standard frames for control commands, whole-image CRC after write).
/// The Python reference confirms T2 maps to `BK7231NFlashHandler`, not `T5AIFlashHandler`.
pub struct T2Spec;

impl ChipSpec for T2Spec {
    fn name(&self) -> &'static str {
        "T2"
    }

    fn baud_switch_delay_ms(&self) -> u8 {
        100
    }

    fn use_extended_frame(&self, _addr: u32) -> bool {
        true
    }

    fn use_extended_flash_mid(&self) -> bool {
        true
    }

    fn use_extended_flash_ops(&self) -> bool {
        true
    }

    fn use_block_erase_64k(&self) -> bool {
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────
// T3
// ─────────────────────────────────────────────────────────────────────────

/// T3 chip specification.
///
/// T3 uses the T5AI protocol variant (extended reset sequence, per-sector CRC,
/// skip blank sectors, post-handshake ChipID read, 20ms baud-switch delay).
/// The Python reference confirms T3 maps to `T5AIFlashHandler`, not `BK7231NFlashHandler`.
pub struct T3Spec;

impl ChipSpec for T3Spec {
    fn name(&self) -> &'static str {
        "T3"
    }

    fn handshake_interval_ms(&self) -> u64 {
        1
    }

    fn baud_switch_delay_ms(&self) -> u8 {
        20
    }

    fn needs_post_handshake(&self) -> bool {
        true
    }

    fn uses_extended_reset_sequence(&self) -> bool {
        true
    }

    fn use_extended_frame(&self, _addr: u32) -> bool {
        true
    }

    fn has_per_sector_crc(&self) -> bool {
        true
    }

    fn skip_blank_sectors(&self) -> bool {
        true
    }

    fn use_extended_flash_mid(&self) -> bool {
        true
    }

    fn use_extended_flash_ops(&self) -> bool {
        true
    }
}

/// Post-handshake: read ChipID for chips that need it (T5AI, T2, etc.).
///
/// Tries each register address in `CHIP_ID_REGS` in order; logs the result.
/// This is a free function (not a trait method) because it needs the generic
/// `Transport<T>` parameter, which would make `ChipSpec` not dyn-compatible.
pub fn post_handshake_read_chip_id<T: IoTransport>(
    transport: &mut Transport<T>,
    chip_name: &str,
) -> Result<(), ProtocolError> {
    const CHIP_ID_REGS: &[u32] = &[0x4401_0004, 0x0080_0000, 0x3401_0004];
    for &reg_addr in CHIP_ID_REGS {
        transport.log(&format!(
            "{chip_name}: reading ChipID from {reg_addr:#010x}"
        ));
        match read_chip_id(transport, reg_addr) {
            Ok(chip_id) => {
                transport.log(&format!("{chip_name}: ChipID = {chip_id:#010x}"));
                return Ok(());
            }
            Err(_) => continue,
        }
    }
    transport.log(&format!(
        "{chip_name}: could not read ChipID (continuing anyway)"
    ));
    Ok(())
}

/// Read ChipID via `ReadReg` command.
fn read_chip_id<T: IoTransport>(
    transport: &mut Transport<T>,
    reg_addr: u32,
) -> Result<u32, ProtocolError> {
    use super::command::{build, parse, CMD_READ_REG};
    use super::frame;

    let payload = build::read_reg(reg_addr);
    let tx = frame::encode_standard(CMD_READ_REG, &payload);
    transport.send_raw(&tx)?;
    let rx = transport.recv_frame(3000)?;
    parse::chip_id_from_read_reg(&rx.data)
}

/// T5AI post-handshake: kept for backwards compatibility — delegates to the shared function.
#[deprecated(note = "use post_handshake_read_chip_id instead")]
pub fn t5ai_post_handshake<T: IoTransport>(
    transport: &mut Transport<T>,
) -> Result<(), ProtocolError> {
    post_handshake_read_chip_id(transport, "T5AI")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bk7231n_spec_getters() {
        let s = Bk7231nSpec;
        assert_eq!(s.name(), "BK7231N");
        // Defaults inherited from the trait.
        assert_eq!(s.initial_baud(), 115200);
        assert_eq!(s.handshake_retries(), 50);
        assert_eq!(s.handshake_interval_ms(), 20);
        assert!(!s.needs_post_handshake());
        assert!(!s.uses_extended_reset_sequence());
        assert!(!s.has_per_sector_crc());
        assert!(!s.skip_blank_sectors());
        assert!(!s.use_extended_erase());
        // Overridden values.
        assert_eq!(s.baud_switch_delay_ms(), 100);
        assert!(s.use_extended_frame(0));
        assert!(s.use_extended_frame(0x1234));
        assert!(s.use_extended_flash_mid());
        assert!(s.use_extended_flash_ops());
        assert!(s.use_block_erase_64k());
    }

    #[test]
    fn t5ai_spec_getters() {
        let s = T5AISpec;
        assert_eq!(s.name(), "T5AI");
        assert_eq!(s.initial_baud(), 115200);
        assert_eq!(s.handshake_retries(), 50);
        assert_eq!(s.handshake_interval_ms(), 1);
        assert_eq!(s.baud_switch_delay_ms(), 20);
        assert!(s.needs_post_handshake());
        assert!(s.uses_extended_reset_sequence());
        assert!(s.use_extended_frame(0));
        assert!(s.use_extended_frame(0xffff_ffff));
        assert!(s.has_per_sector_crc());
        assert!(s.skip_blank_sectors());
        assert!(!s.use_extended_erase());
        assert!(s.use_block_erase_64k());
        assert!(s.use_extended_flash_mid());
        assert!(s.use_extended_flash_ops());
    }

    #[test]
    fn t1_spec_getters() {
        let s = T1Spec;
        assert_eq!(s.name(), "T1");
        assert_eq!(s.handshake_interval_ms(), 1);
        assert_eq!(s.baud_switch_delay_ms(), 20);
        assert!(s.needs_post_handshake());
        assert!(s.uses_extended_reset_sequence());
        assert!(s.use_extended_frame(0));
        assert!(s.has_per_sector_crc());
        assert!(s.skip_blank_sectors());
        assert!(!s.use_extended_erase());
        assert!(s.use_extended_flash_mid());
        assert!(s.use_extended_flash_ops());
        // Inherited default.
        assert!(s.use_block_erase_64k());
    }

    #[test]
    fn t2_spec_getters() {
        let s = T2Spec;
        assert_eq!(s.name(), "T2");
        assert_eq!(s.baud_switch_delay_ms(), 100);
        // Inherited defaults — T2 behaves like BK7231N.
        assert_eq!(s.handshake_interval_ms(), 20);
        assert!(!s.needs_post_handshake());
        assert!(!s.uses_extended_reset_sequence());
        assert!(!s.has_per_sector_crc());
        assert!(!s.skip_blank_sectors());
        // Overrides.
        assert!(s.use_extended_frame(0));
        assert!(s.use_extended_flash_mid());
        assert!(s.use_extended_flash_ops());
        assert!(s.use_block_erase_64k());
    }

    #[test]
    fn t3_spec_getters() {
        let s = T3Spec;
        assert_eq!(s.name(), "T3");
        assert_eq!(s.handshake_interval_ms(), 1);
        assert_eq!(s.baud_switch_delay_ms(), 20);
        assert!(s.needs_post_handshake());
        assert!(s.uses_extended_reset_sequence());
        assert!(s.use_extended_frame(0));
        assert!(s.has_per_sector_crc());
        assert!(s.skip_blank_sectors());
        assert!(s.use_extended_flash_mid());
        assert!(s.use_extended_flash_ops());
        // Inherited defaults.
        assert!(!s.use_extended_erase());
        assert!(s.use_block_erase_64k());
    }

    #[test]
    fn chip_id_reg_tables() {
        let expected = &[0x4401_0004u32, 0x0080_0000, 0x3401_0004];
        assert_eq!(T5AI_CHIP_ID_REGS, expected);
        assert_eq!(T2_CHIP_ID_REGS, expected);
    }

    #[test]
    fn spec_usable_as_trait_object() {
        // Confirms every spec is dyn-compatible (used as &dyn ChipSpec in run_beken).
        let specs: Vec<Box<dyn ChipSpec>> = vec![
            Box::new(Bk7231nSpec),
            Box::new(T5AISpec),
            Box::new(T1Spec),
            Box::new(T2Spec),
            Box::new(T3Spec),
        ];
        let names: Vec<&str> = specs.iter().map(|s| s.name()).collect();
        assert_eq!(names, vec!["BK7231N", "T5AI", "T1", "T2", "T3"]);
    }
}
