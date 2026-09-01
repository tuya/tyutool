//! GD32VW553 download protocol — wire format and one-step-at-a-time device calls.
//!
//! Flashing runs in two stages, both reconstructed from a USB capture of the vendor
//! tool driving a real module (CH340 bridge, `gd32_1.pcapng`, 2026-09-01):
//!
//! 1. **System bootloader** — the AN3155-style USART protocol in mask ROM, spoken at
//!    57600 baud **8E1**. It is used for nothing but uploading [`super::LOADER_BIN`]
//!    into SRAM at [`LOADER_LOAD_ADDR`] and jumping to it.
//! 2. **RAM loader** — the uploaded downloader, spoken at 115200 baud **8N1** and then
//!    at a negotiated transfer baud. It owns erase, program and verify; the image is
//!    streamed to it in XMODEM-shaped frames that each carry their own destination
//!    address.
//!
//! Every constant and frame layout below is what the capture showed on the wire, and
//! the golden-byte tests at the bottom pin the exact encodings the vendor tool used so
//! a refactor cannot quietly change them.

use serialport::{Parity, SerialPort};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::error::FlashError;
use crate::flash_event::FlashEvent;

// ── Stage 1: system bootloader ──────────────────────────────────────────────

/// Baud the mask-ROM bootloader is driven at. It auto-bauds on the sync byte, but the
/// vendor tool picks 57600 and so do we — the ROM is only ever asked to move 15 KiB of
/// loader, so a faster link buys ~nothing and risks the autobaud.
pub const ISP_BAUD: u32 = 57_600;
/// The ROM bootloader frames bytes as 8 data bits, **even** parity, 1 stop bit.
pub const ISP_PARITY: Parity = Parity::Even;

/// Sync byte that starts (and auto-bauds) a bootloader session.
pub const ISP_SYNC: u8 = 0x7F;
/// Bootloader acknowledgement.
pub const ISP_ACK: u8 = 0x79;
/// Bootloader negative acknowledgement. Also what a *second* [`ISP_SYNC`] gets once the
/// session is already up, which is why [`isp_sync`] treats it as "already synced".
pub const ISP_NACK: u8 = 0x1F;

/// Write Memory command code.
pub const ISP_CMD_WRITE_MEMORY: u8 = 0x31;
/// Go (jump to address) command code.
pub const ISP_CMD_GO: u8 = 0x21;

/// SRAM address the RAM loader is written to and entered at.
pub const LOADER_LOAD_ADDR: u32 = 0x2000_2000;

/// Bytes of payload per Write Memory command. The protocol allows 256; the vendor tool
/// uses 240 and the loader blob is an exact multiple of it.
pub const ISP_CHUNK: usize = 240;

// ── Stage 2: RAM loader ─────────────────────────────────────────────────────

/// Baud the RAM loader starts at, before [`loader_set_baud`] moves it.
pub const LOADER_BAUD: u32 = 115_200;
/// The RAM loader frames bytes as 8N1 — note the parity change from stage 1.
pub const LOADER_PARITY: Parity = Parity::None;

/// Loader ping; answered with [`ISP_ACK`] (the loader reuses the ROM's ACK code here).
pub const LOADER_CMD_PING: u8 = 0x75;
/// Read chip id; answered with 4 raw bytes.
pub const LOADER_CMD_CHIP_ID: u8 = 0x20;
/// Erase sectors: `[cmd, offset u32 LE, sector_count u16 LE]`.
pub const LOADER_CMD_ERASE: u8 = 0x17;
/// Set transfer baud: `[cmd, baud u32 LE]`. The host must follow after the ACK.
pub const LOADER_CMD_SET_BAUD: u8 = 0x05;
/// Set frame size: `[cmd, units u8]`, where a frame carries `units * 256` data bytes.
pub const LOADER_CMD_FRAME_SIZE: u8 = 0x07;
/// Verify: `[cmd, offset u24 LE, len u24 LE, algorithm]`; answers with the digest.
pub const LOADER_CMD_VERIFY: u8 = 0x21;
/// Reset the chip into the freshly written application: `[cmd, 0x01]`.
pub const LOADER_CMD_RESET: u8 = 0x22;

/// Verify algorithm selector. `2` is the only value observed, and it returns SHA-256.
pub const LOADER_VERIFY_SHA256: u8 = 0x02;
/// Length of the digest [`LOADER_CMD_VERIFY`] answers with.
pub const LOADER_DIGEST_LEN: usize = 32;

/// XMODEM-shaped data frame header byte.
pub const STX: u8 = 0x02;
/// End of transmission — closes the image stream.
pub const EOT: u8 = 0x04;
/// Loader acknowledgement (XMODEM ACK, *not* [`ISP_ACK`]).
pub const LOADER_ACK: u8 = 0x06;
/// Loader negative acknowledgement — resend the frame.
pub const LOADER_NAK: u8 = 0x15;
/// Loader abort.
pub const LOADER_CAN: u8 = 0x18;

/// Frame size unit: [`LOADER_CMD_FRAME_SIZE`] counts 256-byte units.
pub const FRAME_UNIT: usize = 256;
/// Units per frame the vendor tool negotiates (10 → 2560 data bytes per frame).
pub const FRAME_UNITS: u8 = 10;
/// Data bytes carried by one frame.
pub const FRAME_DATA_LEN: usize = FRAME_UNIT * FRAME_UNITS as usize;
/// Fill byte for the tail of the last frame — the same value an erased sector holds,
/// so the padding is a no-op for the device.
pub const PAD_BYTE: u8 = 0xFF;

/// Where flash is mapped in the CPU address space. Frame addresses are absolute
/// (`FLASH_BASE + offset`); erase and verify take a bare offset.
pub const FLASH_BASE: u32 = 0x0800_0000;
/// Flash erase granularity.
pub const SECTOR_SIZE: u32 = 0x1000;

/// Largest offset/length the verify command's 24-bit fields can express.
const U24_MAX: u32 = 0x00FF_FFFF;

// ── Frame builders (pure) ───────────────────────────────────────────────────

/// A bootloader command byte followed by its complement, as the ROM protocol requires.
pub fn isp_command(code: u8) -> [u8; 2] {
    [code, !code]
}

/// A bootloader address argument: big-endian address plus an XOR checksum byte.
pub fn isp_address(addr: u32) -> [u8; 5] {
    let b = addr.to_be_bytes();
    [b[0], b[1], b[2], b[3], b[0] ^ b[1] ^ b[2] ^ b[3]]
}

/// A Write Memory payload block: `len-1`, the data, then the XOR of both.
///
/// Panics if `chunk` is empty or longer than 256 bytes — the length is encoded in one
/// byte, so no other size can be expressed, and every caller slices by [`ISP_CHUNK`].
pub fn isp_write_block(chunk: &[u8]) -> Vec<u8> {
    assert!(
        !chunk.is_empty() && chunk.len() <= 256,
        "Write Memory block must hold 1..=256 bytes, got {}",
        chunk.len()
    );
    let mut out = Vec::with_capacity(chunk.len() + 2);
    let n = (chunk.len() - 1) as u8;
    out.push(n);
    out.extend_from_slice(chunk);
    out.push(chunk.iter().fold(n, |acc, b| acc ^ b));
    out
}

/// Erase `sectors` sectors of [`SECTOR_SIZE`] starting at flash `offset`.
pub fn loader_erase_command(offset: u32, sectors: u16) -> [u8; 7] {
    let o = offset.to_le_bytes();
    let s = sectors.to_le_bytes();
    [LOADER_CMD_ERASE, o[0], o[1], o[2], o[3], s[0], s[1]]
}

/// Ask the loader to move to `baud`.
pub fn loader_set_baud_command(baud: u32) -> [u8; 5] {
    let b = baud.to_le_bytes();
    [LOADER_CMD_SET_BAUD, b[0], b[1], b[2], b[3]]
}

/// Ask the loader for `units * 256`-byte data frames.
pub fn loader_frame_size_command(units: u8) -> [u8; 2] {
    [LOADER_CMD_FRAME_SIZE, units]
}

/// Ask the loader to digest `len` bytes of flash from `offset`.
///
/// Both fields are 24 bits wide on the wire, which is why this can fail: a range past
/// 16 MiB has no encoding. Every GD32VW553 part is well inside that.
pub fn loader_verify_command(offset: u32, len: u32) -> Result<[u8; 8], FlashError> {
    if offset > U24_MAX || len > U24_MAX {
        return Err(FlashError::InvalidJob(format!(
            "GD32VW553: verify range 0x{offset:08X}+0x{len:08X} is past the 16 MiB the protocol can address"
        )));
    }
    let o = offset.to_le_bytes();
    let l = len.to_le_bytes();
    Ok([
        LOADER_CMD_VERIFY,
        o[0],
        o[1],
        o[2],
        l[0],
        l[1],
        l[2],
        LOADER_VERIFY_SHA256,
    ])
}

/// One image frame: `STX`, sequence, its complement, the absolute destination address,
/// [`FRAME_DATA_LEN`] data bytes, and a one-byte sum over address plus data.
///
/// `data` is padded with [`PAD_BYTE`] when short (only the last frame ever is).
/// Panics if `data` overflows a frame — callers chunk by [`FRAME_DATA_LEN`].
pub fn data_frame(seq: u8, addr: u32, data: &[u8]) -> Vec<u8> {
    assert!(
        data.len() <= FRAME_DATA_LEN,
        "frame payload must be <= {FRAME_DATA_LEN} bytes, got {}",
        data.len()
    );
    let mut frame = Vec::with_capacity(3 + 4 + FRAME_DATA_LEN + 1);
    frame.push(STX);
    frame.push(seq);
    frame.push(!seq);
    frame.extend_from_slice(&addr.to_le_bytes());
    frame.extend_from_slice(data);
    frame.resize(3 + 4 + FRAME_DATA_LEN, PAD_BYTE);
    let checksum = frame[3..].iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
    frame.push(checksum);
    frame
}

/// Sequence number of the `index`-th frame: 1-based, wrapping through 0.
pub fn frame_seq(index: usize) -> u8 {
    (index as u8).wrapping_add(1)
}

/// Sectors needed to hold `len` bytes.
pub fn sector_count(len: u32) -> u32 {
    len.div_ceil(SECTOR_SIZE)
}

// ── I/O seam ────────────────────────────────────────────────────────────────

/// Byte-level serial access this protocol needs, so the whole flow can be driven
/// against a scripted device in tests.
///
/// Deliberately not `plugins::beken::transport::IoTransport`: that trait lives inside
/// the Beken protocol module next to its framing and error type, and has no parity
/// control — which stage 1 here requires, the ROM bootloader being 8E1 and the RAM
/// loader 8N1. If a third plugin ever needs a seam, lift one of the two into a shared
/// module rather than adding another.
pub trait Gd32Io: Send {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()>;
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize>;
    /// Re-frame the link. Both fields change between stage 1 and stage 2.
    fn reconfigure(&mut self, baud: u32, parity: Parity) -> io::Result<()>;
    fn set_dtr(&mut self, level: bool) -> io::Result<()>;
    fn set_rts(&mut self, level: bool) -> io::Result<()>;
    /// Drop whatever the device sent that we have not read yet.
    fn clear_input(&mut self) -> io::Result<()>;
}

impl Gd32Io for Box<dyn SerialPort> {
    fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        io::Write::write_all(self, data)?;
        io::Write::flush(self)
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        io::Read::read(self, buf)
    }

    fn reconfigure(&mut self, baud: u32, parity: Parity) -> io::Result<()> {
        SerialPort::set_baud_rate(&mut **self, baud).map_err(io::Error::other)?;
        SerialPort::set_parity(&mut **self, parity).map_err(io::Error::other)
    }

    fn set_dtr(&mut self, level: bool) -> io::Result<()> {
        self.write_data_terminal_ready(level)
            .map_err(io::Error::other)
    }

    fn set_rts(&mut self, level: bool) -> io::Result<()> {
        self.write_request_to_send(level).map_err(io::Error::other)
    }

    fn clear_input(&mut self) -> io::Result<()> {
        self.clear(serialport::ClearBuffer::Input)
            .map_err(io::Error::other)
    }
}

/// Open `port_name` framed the way stage 1 needs it.
pub fn open_port(
    port_name: &str,
    baud: u32,
    parity: Parity,
) -> Result<Box<dyn SerialPort>, FlashError> {
    serialport::new(port_name, baud)
        .parity(parity)
        // Poll timeout: every wait below is a deadline loop on top of it, so this is how
        // often the cancel flag is checked, not how long a step may take. It also bounds
        // a write, so it has to stay above the wire time of the largest frame at the
        // slowest link this plugin uses (2568 bytes at 115200 baud ≈ 223 ms).
        .timeout(Duration::from_millis(500))
        .open()
        .map_err(|e| FlashError::Plugin(format!("cannot open port {port_name}: {e}")))
}

// ── Device conversation ─────────────────────────────────────────────────────

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

/// Fill `buf` within `timeout`, polling the cancel flag while it waits.
fn read_exact_within(
    io: &mut dyn Gd32Io,
    buf: &mut [u8],
    timeout: Duration,
    cancel: &AtomicBool,
    what: &str,
) -> Result<(), FlashError> {
    let deadline = Instant::now() + timeout;
    let mut filled = 0;
    while filled < buf.len() {
        if cancelled(cancel) {
            return Err(FlashError::Cancelled);
        }
        match io.read(&mut buf[filled..]) {
            // A port at rest reports a timeout rather than a short read, but do not
            // spin the CPU on an implementation that returns zero instead.
            Ok(0) => sleep(Duration::from_millis(1)),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(FlashError::Io(e)),
        }
        if filled < buf.len() && Instant::now() >= deadline {
            return Err(FlashError::Plugin(format!(
                "GD32VW553: no answer to {what} — the device sent {filled} of {} byte(s)",
                buf.len()
            )));
        }
    }
    Ok(())
}

/// Read one byte and require it to be `expected`.
fn expect_byte(
    io: &mut dyn Gd32Io,
    expected: u8,
    timeout: Duration,
    cancel: &AtomicBool,
    what: &str,
) -> Result<(), FlashError> {
    let mut got = [0u8; 1];
    read_exact_within(io, &mut got, timeout, cancel, what)?;
    if got[0] == expected {
        return Ok(());
    }
    Err(FlashError::Plugin(format!(
        "GD32VW553: {what} was answered with 0x{:02X}, expected 0x{expected:02X}",
        got[0]
    )))
}

/// Read until `wanted` shows up, discarding anything before it.
///
/// Used only where the device legitimately has stale bytes queued: the ROM emits a
/// second ACK as it jumps into the loader.
fn wait_for_byte(
    io: &mut dyn Gd32Io,
    wanted: u8,
    timeout: Duration,
    cancel: &AtomicBool,
    what: &str,
) -> Result<(), FlashError> {
    let deadline = Instant::now() + timeout;
    let mut got = [0u8; 1];
    loop {
        if cancelled(cancel) {
            return Err(FlashError::Cancelled);
        }
        match io.read(&mut got) {
            Ok(1) if got[0] == wanted => return Ok(()),
            Ok(0) => sleep(Duration::from_millis(1)),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::TimedOut => {}
            Err(e) => return Err(FlashError::Io(e)),
        }
        if Instant::now() >= deadline {
            return Err(FlashError::Plugin(format!(
                "GD32VW553: no answer to {what}"
            )));
        }
    }
}

/// Pulse the UART control lines so the chip comes out of reset in ISP mode.
///
/// Wiring the vendor tool assumes: **DTR is RESET, RTS is BOOT0**. BOOT0 has to be
/// asserted before reset is released, because that is when the chip samples it.
///
/// On a board that does not wire those two lines — a bare module, or one where BOOT is a
/// button — this is a no-op and the operator enters boot mode by hand; [`isp_sync`] keeps
/// retrying for long enough that they can, and says so.
pub fn enter_isp_mode(io: &mut dyn Gd32Io) -> Result<(), FlashError> {
    let line_err = |e: io::Error| FlashError::Plugin(format!("GD32VW553: control line: {e}"));
    io.set_dtr(true).map_err(line_err)?; // hold in reset
    io.set_rts(false).map_err(line_err)?;
    sleep(Duration::from_millis(50));
    io.set_rts(true).map_err(line_err)?; // assert BOOT0 while still in reset
    sleep(Duration::from_millis(100));
    io.set_dtr(false).map_err(line_err)?; // release reset — BOOT0 is sampled here
    sleep(Duration::from_millis(100));
    io.set_rts(false).map_err(line_err)?;
    io.clear_input().map_err(line_err)?;
    Ok(())
}

/// Reset into ISP mode and sync with the ROM bootloader, retrying until [`SYNC_WINDOW`]
/// runs out.
///
/// One sync byte per reset, as the vendor tool sends: when the board really is in boot
/// mode the ROM answers the first one in ~2 ms, and every successful run on hardware has.
///
/// The retry loop is not about a flaky link, it is about **the board not being in boot
/// mode yet** — the common case on a board whose BOOT0 is a button rather than a wire to
/// RTS. So after the first silent attempt the caller's `progress` gets a warning saying
/// what to do, and the loop keeps resetting for half a minute while the operator does it.
/// Holding the boot button through those retries is exactly the manual procedure, since
/// each retry pulses reset for them.
pub fn isp_sync(
    io: &mut dyn Gd32Io,
    cancel: &AtomicBool,
    progress: &dyn Fn(FlashEvent),
) -> Result<(), FlashError> {
    /// How long to keep offering to sync before giving up.
    const SYNC_WINDOW: Duration = Duration::from_secs(30);
    /// How long one reset waits for the ROM's answer.
    const REPLY: Duration = Duration::from_secs(1);

    let deadline = Instant::now() + SYNC_WINDOW;
    let mut warned = false;
    for attempt in 1u32.. {
        if cancelled(cancel) {
            return Err(FlashError::Cancelled);
        }
        enter_isp_mode(io)?;
        io.write_all(&[ISP_SYNC])?;
        let mut got = [0u8; 1];
        match read_exact_within(io, &mut got, REPLY, cancel, "the sync byte") {
            // A NACK means the ROM is up and already synced (a repeat sync byte always
            // draws one), which serves just as well as an ACK for what follows.
            Ok(()) if got[0] == ISP_ACK || got[0] == ISP_NACK => {
                log::debug!("synced on attempt {attempt}");
                return Ok(());
            }
            Ok(()) => log::debug!("sync attempt {attempt}: unexpected 0x{:02X}", got[0]),
            Err(FlashError::Cancelled) => return Err(FlashError::Cancelled),
            Err(e) => log::debug!("sync attempt {attempt}: {e}"),
        }
        if !warned {
            warned = true;
            progress(FlashEvent::Warning {
                message: "GD32VW553: the boot ROM is not answering — put the board in boot \
                          mode (hold BOOT0/BOOT low, then reset or power-cycle) and keep it \
                          held; this keeps retrying for 30 s"
                    .into(),
            });
        }
        if Instant::now() >= deadline {
            log::warn!("giving up after {attempt} sync attempt(s)");
            break;
        }
    }
    Err(FlashError::Plugin(
        "GD32VW553: the boot ROM did not answer — the board was not in boot mode. Hold \
         BOOT0/BOOT low and reset or power-cycle it, or wire DTR to RESET and RTS to BOOT0 \
         so the tool can do it itself"
            .into(),
    ))
}

/// Write `data` to SRAM at `addr` with repeated Write Memory commands.
pub fn isp_write_memory(
    io: &mut dyn Gd32Io,
    addr: u32,
    data: &[u8],
    cancel: &AtomicBool,
) -> Result<(), FlashError> {
    const REPLY: Duration = Duration::from_secs(2);
    for (i, chunk) in data.chunks(ISP_CHUNK).enumerate() {
        if cancelled(cancel) {
            return Err(FlashError::Cancelled);
        }
        let at = addr + (i * ISP_CHUNK) as u32;
        io.write_all(&isp_command(ISP_CMD_WRITE_MEMORY))?;
        expect_byte(io, ISP_ACK, REPLY, cancel, "the Write Memory command")?;
        io.write_all(&isp_address(at))?;
        expect_byte(io, ISP_ACK, REPLY, cancel, "the Write Memory address")?;
        io.write_all(&isp_write_block(chunk))?;
        expect_byte(io, ISP_ACK, REPLY, cancel, "a Write Memory block")?;
    }
    Ok(())
}

/// Jump to `addr`. The ROM ACKs the address and then hands over.
pub fn isp_go(io: &mut dyn Gd32Io, addr: u32, cancel: &AtomicBool) -> Result<(), FlashError> {
    const REPLY: Duration = Duration::from_secs(2);
    io.write_all(&isp_command(ISP_CMD_GO))?;
    expect_byte(io, ISP_ACK, REPLY, cancel, "the Go command")?;
    io.write_all(&isp_address(addr))?;
    expect_byte(io, ISP_ACK, REPLY, cancel, "the Go address")?;
    Ok(())
}

/// Re-frame the link for the RAM loader and shake hands with it.
///
/// Returns the four chip-id bytes the loader reports, for the log.
pub fn loader_handshake(io: &mut dyn Gd32Io, cancel: &AtomicBool) -> Result<[u8; 4], FlashError> {
    io.reconfigure(LOADER_BAUD, LOADER_PARITY).map_err(|e| {
        FlashError::Plugin(format!("GD32VW553: cannot switch to the loader link: {e}"))
    })?;
    // The loader needs a moment to reach its command loop after the jump.
    sleep(Duration::from_millis(100));
    // The ROM answers the Go address with *two* ACKs; one is still queued here, and
    // reading it as the ping's reply would push every later read one byte out of step.
    io.clear_input()
        .map_err(|e| FlashError::Plugin(format!("GD32VW553: cannot flush the port: {e}")))?;

    io.write_all(&[LOADER_CMD_PING])?;
    // Skip anything the jump left behind rather than mistaking it for the reply.
    wait_for_byte(
        io,
        ISP_ACK,
        Duration::from_secs(2),
        cancel,
        "the RAM loader ping",
    )?;

    io.write_all(&[LOADER_CMD_CHIP_ID])?;
    let mut id = [0u8; 4];
    read_exact_within(
        io,
        &mut id,
        Duration::from_secs(2),
        cancel,
        "the chip id request",
    )?;
    Ok(id)
}

/// Erase `sectors` sectors from flash `offset`.
///
/// `timeout` has to cover the whole erase: the vendor tool spent 15 s on 317 sectors.
pub fn loader_erase(
    io: &mut dyn Gd32Io,
    offset: u32,
    sectors: u16,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<(), FlashError> {
    io.write_all(&loader_erase_command(offset, sectors))?;
    expect_byte(io, LOADER_ACK, timeout, cancel, "the erase command")
}

/// Move the link to `baud`: tell the loader, then follow it.
pub fn loader_set_baud(
    io: &mut dyn Gd32Io,
    baud: u32,
    cancel: &AtomicBool,
) -> Result<(), FlashError> {
    io.write_all(&loader_set_baud_command(baud))?;
    expect_byte(
        io,
        LOADER_ACK,
        Duration::from_secs(2),
        cancel,
        "the baud rate change",
    )?;
    io.reconfigure(baud, LOADER_PARITY)
        .map_err(|e| FlashError::Plugin(format!("GD32VW553: cannot switch to {baud} baud: {e}")))?;
    // Let the loader finish re-framing its own UART before the first frame lands.
    sleep(Duration::from_millis(50));
    Ok(())
}

/// Negotiate the image frame size (`units * 256` data bytes).
pub fn loader_set_frame_size(
    io: &mut dyn Gd32Io,
    units: u8,
    cancel: &AtomicBool,
) -> Result<(), FlashError> {
    io.write_all(&loader_frame_size_command(units))?;
    expect_byte(
        io,
        LOADER_ACK,
        Duration::from_secs(2),
        cancel,
        "the frame size",
    )
}

/// Stream `image` to flash `offset`, then close the transfer with EOT.
///
/// `progress` is called with the byte count sent so far after every frame.
pub fn send_image(
    io: &mut dyn Gd32Io,
    offset: u32,
    image: &[u8],
    cancel: &AtomicBool,
    progress: &dyn Fn(usize),
) -> Result<(), FlashError> {
    const FRAME_REPLY: Duration = Duration::from_secs(10);
    const RETRIES: u8 = 3;

    for (index, chunk) in image.chunks(FRAME_DATA_LEN).enumerate() {
        if cancelled(cancel) {
            return Err(FlashError::Cancelled);
        }
        let addr = FLASH_BASE + offset + (index * FRAME_DATA_LEN) as u32;
        let frame = data_frame(frame_seq(index), addr, chunk);

        let mut attempt = 0u8;
        loop {
            io.write_all(&frame)?;
            let mut got = [0u8; 1];
            read_exact_within(io, &mut got, FRAME_REPLY, cancel, "an image frame")?;
            match got[0] {
                LOADER_ACK => break,
                LOADER_NAK if attempt < RETRIES => {
                    attempt += 1;
                    log::warn!(
                        "frame {index} at 0x{addr:08X} NAKed, resending ({attempt}/{RETRIES})"
                    );
                }
                LOADER_CAN => {
                    return Err(FlashError::Plugin(format!(
                        "GD32VW553: the device aborted the transfer at 0x{addr:08X}"
                    )))
                }
                other => {
                    return Err(FlashError::Plugin(format!(
                        "GD32VW553: the frame at 0x{addr:08X} was answered with 0x{other:02X}"
                    )))
                }
            }
        }
        progress((index * FRAME_DATA_LEN + chunk.len()).min(image.len()));
    }

    io.write_all(&[EOT])?;
    expect_byte(
        io,
        LOADER_ACK,
        Duration::from_secs(10),
        cancel,
        "the end of transmission",
    )
}

/// Ask the device to digest `len` bytes of flash from `offset`.
pub fn loader_verify(
    io: &mut dyn Gd32Io,
    offset: u32,
    len: u32,
    cancel: &AtomicBool,
) -> Result<[u8; LOADER_DIGEST_LEN], FlashError> {
    io.write_all(&loader_verify_command(offset, len)?)?;
    let mut digest = [0u8; LOADER_DIGEST_LEN];
    read_exact_within(
        io,
        &mut digest,
        Duration::from_secs(30),
        cancel,
        "the verify request",
    )?;
    Ok(digest)
}

/// Reset the chip so it boots what was just written. The loader does not answer.
pub fn loader_reset(io: &mut dyn Gd32Io) -> Result<(), FlashError> {
    io.write_all(&[LOADER_CMD_RESET, 0x01])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every `hex!` below is a literal the vendor tool put on the wire in
    // `gd32_1.pcapng` while flashing a 1_295_688-byte image at offset 0. They are the
    // only proof we have of the encodings, so they are asserted byte for byte.

    #[test]
    fn isp_command_appends_the_complement() {
        assert_eq!(isp_command(ISP_CMD_WRITE_MEMORY), [0x31, 0xCE]);
        assert_eq!(isp_command(ISP_CMD_GO), [0x21, 0xDE]);
    }

    #[test]
    fn isp_address_is_big_endian_with_an_xor_checksum() {
        // Capture: `20 00 20 00 00` for the loader's load address.
        assert_eq!(
            isp_address(LOADER_LOAD_ADDR),
            [0x20, 0x00, 0x20, 0x00, 0x00]
        );
        // Capture: `20 00 20 f0 f0` for the second chunk.
        assert_eq!(isp_address(0x2000_20F0), [0x20, 0x00, 0x20, 0xF0, 0xF0]);
    }

    #[test]
    fn isp_write_block_is_length_minus_one_data_and_xor() {
        let block = isp_write_block(&[0xD5, 0xA2, 0x01]);
        assert_eq!(block[0], 2, "length is encoded as len-1");
        assert_eq!(&block[1..4], &[0xD5, 0xA2, 0x01]);
        assert_eq!(block[4], 2 ^ 0xD5 ^ 0xA2 ^ 0x01);
        // A full-size chunk is the 240-byte one the vendor tool uses.
        assert_eq!(isp_write_block(&[0u8; ISP_CHUNK]).len(), ISP_CHUNK + 2);
        assert_eq!(isp_write_block(&[0u8; ISP_CHUNK])[0], 0xEF);
    }

    #[test]
    #[should_panic(expected = "1..=256 bytes")]
    fn isp_write_block_rejects_an_empty_chunk() {
        isp_write_block(&[]);
    }

    #[test]
    fn erase_command_matches_the_capture() {
        // 317 sectors = ceil(1_295_688 / 4096), erasing exactly the image footprint.
        assert_eq!(
            loader_erase_command(0, 317),
            [0x17, 0x00, 0x00, 0x00, 0x00, 0x3D, 0x01]
        );
    }

    #[test]
    fn set_baud_command_matches_the_capture() {
        assert_eq!(
            loader_set_baud_command(2_000_000),
            [0x05, 0x80, 0x84, 0x1E, 0x00]
        );
    }

    #[test]
    fn frame_size_command_matches_the_capture() {
        assert_eq!(loader_frame_size_command(FRAME_UNITS), [0x07, 0x0A]);
        assert_eq!(FRAME_DATA_LEN, 2560);
    }

    #[test]
    fn verify_command_matches_the_capture() {
        assert_eq!(
            loader_verify_command(0, 1_295_688).unwrap(),
            [0x21, 0x00, 0x00, 0x00, 0x48, 0xC5, 0x13, 0x02]
        );
    }

    #[test]
    fn verify_command_rejects_a_range_past_the_24_bit_fields() {
        assert!(loader_verify_command(0, 0x0100_0000).is_err());
        assert!(loader_verify_command(0x0100_0000, 1).is_err());
        assert!(loader_verify_command(U24_MAX, U24_MAX).is_ok());
    }

    #[test]
    fn data_frame_header_and_length_match_the_capture() {
        let frame = data_frame(1, FLASH_BASE, &[0u8; FRAME_DATA_LEN]);
        assert_eq!(
            frame.len(),
            2568,
            "3 header + 4 address + 2560 data + 1 sum"
        );
        // Capture, first frame: `02 01 fe 00 00 00 08`.
        assert_eq!(&frame[..7], &[0x02, 0x01, 0xFE, 0x00, 0x00, 0x00, 0x08]);
        // Second frame addresses one frame further in: `02 02 fd 00 0a 00 08`.
        let second = data_frame(2, FLASH_BASE + FRAME_DATA_LEN as u32, &[0u8; 8]);
        assert_eq!(&second[..7], &[0x02, 0x02, 0xFD, 0x00, 0x0A, 0x00, 0x08]);
    }

    #[test]
    fn data_frame_checksum_sums_address_and_data() {
        let frame = data_frame(7, 0x0801_0000, &[0xAA; FRAME_DATA_LEN]);
        let expected = frame[3..frame.len() - 1]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_add(*b));
        assert_eq!(*frame.last().unwrap(), expected);
        // The sequence bytes are outside the checksum: two frames with the same
        // address and payload but different sequence numbers share one.
        let other = data_frame(9, 0x0801_0000, &[0xAA; FRAME_DATA_LEN]);
        assert_eq!(frame.last(), other.last());
    }

    #[test]
    fn data_frame_pads_a_short_tail_with_erased_flash() {
        let frame = data_frame(1, FLASH_BASE, &[0x11, 0x22]);
        assert_eq!(frame.len(), 2568);
        assert_eq!(&frame[7..9], &[0x11, 0x22]);
        assert!(frame[9..frame.len() - 1].iter().all(|&b| b == PAD_BYTE));
    }

    #[test]
    fn frame_sequence_is_one_based_and_wraps_through_zero() {
        assert_eq!(frame_seq(0), 1);
        assert_eq!(frame_seq(254), 255);
        assert_eq!(frame_seq(255), 0);
        assert_eq!(frame_seq(256), 1);
    }

    #[test]
    fn sector_count_rounds_up() {
        assert_eq!(sector_count(0), 0);
        assert_eq!(sector_count(1), 1);
        assert_eq!(sector_count(SECTOR_SIZE), 1);
        assert_eq!(sector_count(SECTOR_SIZE + 1), 2);
        assert_eq!(sector_count(1_295_688), 317); // the capture's image
    }
}
