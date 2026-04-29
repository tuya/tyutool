//! Flash MID (JEDEC Manufacturer ID) lookup table.
//!
//! Each entry describes the SPI-NOR flash chip on the module:
//! capacity, sector/block size, and write-protection register layout.
//!
//! Source: Python `tyutool/flash/bk7231n/protocol.py` `FLASH.TABLE`
//! and `tyutool/flash/t5/flash_info.py`.

// ─────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────

/// Write-protection Status Register configuration for a flash chip.
#[derive(Debug, Clone, Copy)]
pub struct WriteProtectConfig {
    /// SPI command to **read** the status register (e.g. `0x05` for SR1).
    pub read_sr_cmds: &'static [u8],
    /// SPI command to **write** the status register (e.g. `0x01` for SR1).
    pub write_sr_cmds: &'static [u8],
    /// Bitmask of the protection bits inside the SR value(s).
    pub protect_mask: u16,
    /// Value to write to **disable** write protection (usually `0x0000`).
    pub unprotect_value: u16,
    /// Value to write to **enable** write protection.
    pub protect_value: u16,
    /// Number of SR bytes used (1 or 2).
    pub sr_bytes: u8,
}

/// Flash chip parameters indexed by JEDEC MID.
#[derive(Debug, Clone, Copy)]
pub struct FlashParams {
    /// 3-byte JEDEC MID (manufacturer + device ID), stored as `u32` (MSB unused).
    pub mid: u32,
    /// Human-readable IC name.
    pub name: &'static str,
    /// Vendor / manufacturer name.
    pub vendor: &'static str,
    /// Total flash capacity in **bytes** (e.g. `2 * 1024 * 1024` for 2 MiB).
    pub total_size: u32,
    /// Erase sector size (always 4096 for SPI-NOR).
    pub sector_size: u32,
    /// Erase block size (typically 65536 = 64 KiB).
    pub block_size: u32,
    /// Write-protection register config, if known.
    pub wp: Option<WriteProtectConfig>,
}

// ─────────────────────────────────────────────────────────────────────────
// Static SR command arrays (shared by many entries)
// ─────────────────────────────────────────────────────────────────────────

static SR1_READ: &[u8] = &[0x05];
static SR1_WRITE: &[u8] = &[0x01];
static SR12_READ: &[u8] = &[0x05, 0x35];
static SR12_WRITE: &[u8] = &[0x01, 0x31];
static SR12_WRITE_SINGLE: &[u8] = &[0x01]; // write both SRs with one 0x01 cmd (16-bit)
static SR12_READ_MXIC: &[u8] = &[0x05, 0x15]; // MXIC uses 0x15 instead of 0x35

// ─────────────────────────────────────────────────────────────────────────
// Helper macros for table entries
// ─────────────────────────────────────────────────────────────────────────

/// Shorthand for a 1-SR entry with simple protect mask.
const fn wp1(mask: u16, protect: u16) -> Option<WriteProtectConfig> {
    Some(WriteProtectConfig {
        read_sr_cmds: SR1_READ,
        write_sr_cmds: SR1_WRITE,
        protect_mask: mask,
        unprotect_value: 0,
        protect_value: protect,
        sr_bytes: 1,
    })
}

/// Shorthand for a 2-SR entry with dual-register protect.
const fn wp2(
    read_cmds: &'static [u8],
    write_cmds: &'static [u8],
    mask: u16,
    protect: u16,
) -> Option<WriteProtectConfig> {
    Some(WriteProtectConfig {
        read_sr_cmds: read_cmds,
        write_sr_cmds: write_cmds,
        protect_mask: mask,
        unprotect_value: 0,
        protect_value: protect,
        sr_bytes: 2,
    })
}

// ─────────────────────────────────────────────────────────────────────────
// Flash parameter table
// ─────────────────────────────────────────────────────────────────────────

/// Combined flash table covering BK7231N and T5 supported chips.
///
/// MIDs are 3-byte JEDEC IDs; the table is sorted by MID for binary search.
/// Sizes are in bytes (e.g. `2 * MB` = 2 MiB).
const KB: u32 = 1024;
const MB: u32 = 1024 * 1024;

static FLASH_TABLE: &[FlashParams] = &[
    // ── GigaDevice (GD) ────────────────────────────────────────────
    FlashParams {
        mid: 0x134051,
        name: "MD25D40D",
        vendor: "GD",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x1340c8,
        name: "GD25Q41B",
        vendor: "GD",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x144051,
        name: "MD25D80D",
        vendor: "GD",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x1440c8,
        name: "GD25Q80C",
        vendor: "GD",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1464c8,
        name: "GD25WD80E",
        vendor: "GD",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x1540c8,
        name: "GD25Q16C",
        vendor: "GD",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1565c8,
        name: "GD25WQ16E",
        vendor: "GD",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1640c8,
        name: "GD25Q32C",
        vendor: "GD",
        total_size: 32 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1665c8,
        name: "GD25WQ32E",
        vendor: "GD",
        total_size: 32 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1740c8,
        name: "GD25Q64C",
        vendor: "GD",
        total_size: 64 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1765c8,
        name: "GD25WQ64E",
        vendor: "GD",
        total_size: 64 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1840c8,
        name: "GD25Q128C",
        vendor: "GD",
        total_size: 128 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    // ── XTX ─────────────────────────────────────────────────────────
    FlashParams {
        mid: 0x14405e,
        name: "PN25F08B",
        vendor: "XTX",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x13311c,
        name: "PN25F04B",
        vendor: "XTX",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x15400b,
        name: "XT25F16B",
        vendor: "XTX",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x16400b,
        name: "XT25F32B",
        vendor: "XTX",
        total_size: 32 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x17400b,
        name: "XT25F64B",
        vendor: "XTX",
        total_size: 64 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x17600b,
        name: "XT25Q64B",
        vendor: "XTX",
        total_size: 64 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── MXIC ────────────────────────────────────────────────────────
    FlashParams {
        mid: 0x1323c2,
        name: "MX25V4035F",
        vendor: "MXIC",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ_MXIC, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1423c2,
        name: "MX25V8035F",
        vendor: "MXIC",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ_MXIC, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1523c2,
        name: "MX25V1635F",
        vendor: "MXIC",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ_MXIC, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── Puya ────────────────────────────────────────────────────────
    FlashParams {
        mid: 0x124485,
        name: "PY25D22U",
        vendor: "Puya",
        total_size: 2 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x124585,
        name: "PY25D24U",
        vendor: "Puya",
        total_size: 2 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x136085,
        name: "PY25Q40H",
        vendor: "Puya",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x146085,
        name: "PY25Q80H",
        vendor: "Puya",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x152085,
        name: "P25Q16HB",
        vendor: "Puya",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x154285,
        name: "PY25Q16SH",
        vendor: "Puya",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x156085,
        name: "PY25Q16H",
        vendor: "Puya",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x166085,
        name: "PY25Q32H",
        vendor: "Puya",
        total_size: 32 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x176085,
        name: "PY25Q64H",
        vendor: "Puya",
        total_size: 64 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── BoyaMicro (BY) ──────────────────────────────────────────────
    FlashParams {
        mid: 0x1340e0,
        name: "BY25Q40A",
        vendor: "BY",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1440e0,
        name: "BY25Q80A",
        vendor: "BY",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── Winbond (WB) ────────────────────────────────────────────────
    FlashParams {
        mid: 0x1840ef,
        name: "W25Q128JV",
        vendor: "Winbond",
        total_size: 128 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── ESMT ────────────────────────────────────────────────────────
    FlashParams {
        mid: 0x15701c,
        name: "EN25QH16B",
        vendor: "ESMT",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x16411c,
        name: "EN25QH32A",
        vendor: "ESMT",
        total_size: 32 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp1(0x0f, 0x07),
    },
    FlashParams {
        mid: 0x16611c,
        name: "EN25QH32A",
        vendor: "ESMT",
        total_size: 32 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── TianHe (TH) ─────────────────────────────────────────────────
    FlashParams {
        mid: 0x1260eb,
        name: "TH25D20HA",
        vendor: "TH",
        total_size: 2 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1360cd,
        name: "TH25Q40HB",
        vendor: "TH",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1460cd,
        name: "TH25Q80HB",
        vendor: "TH",
        total_size: 8 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1560eb,
        name: "TH25Q16HB",
        vendor: "TH",
        total_size: 16 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1760eb,
        name: "TH25Q64HA",
        vendor: "TH",
        total_size: 64 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE, 0x407c, 0x0007),
    },
    // ── UC ──────────────────────────────────────────────────────────
    FlashParams {
        mid: 0x1260b3,
        name: "UC25HQ20",
        vendor: "UC",
        total_size: 2 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1360b3,
        name: "UC25HQ40",
        vendor: "UC",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    // ── GT ──────────────────────────────────────────────────────────
    FlashParams {
        mid: 0x1240c4,
        name: "GT25Q20D",
        vendor: "GT",
        total_size: 2 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
    FlashParams {
        mid: 0x1340c4,
        name: "GT25Q40D",
        vendor: "GT",
        total_size: 4 * MB / 8,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: wp2(SR12_READ, SR12_WRITE_SINGLE, 0x407c, 0x0007),
    },
];

// ─────────────────────────────────────────────────────────────────────────
// Lookup
// ─────────────────────────────────────────────────────────────────────────

/// Look up flash parameters by JEDEC MID.
pub fn lookup(mid: u32) -> Option<&'static FlashParams> {
    FLASH_TABLE.iter().find(|p| p.mid == mid)
}

/// Return a fallback `FlashParams` for an unknown MID.
///
/// Uses conservative defaults (2 MiB, standard 4K/64K, no WP config).
pub fn fallback(mid: u32) -> FlashParams {
    FlashParams {
        mid,
        name: "Unknown",
        vendor: "Unknown",
        total_size: 2 * MB,
        sector_size: 4 * KB,
        block_size: 64 * KB,
        wp: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_gd25q80c() {
        let p = lookup(0x1440c8).unwrap();
        assert_eq!(p.name, "GD25Q80C");
        assert_eq!(p.vendor, "GD");
        assert_eq!(p.total_size, 1024 * 1024); // 8 Mbit = 1 MiB
        assert_eq!(p.sector_size, 4096);
        assert_eq!(p.block_size, 65536);
        let wp = p.wp.unwrap();
        assert_eq!(wp.sr_bytes, 2);
    }

    #[test]
    fn lookup_w25q128jv() {
        let p = lookup(0x1840ef).unwrap();
        assert_eq!(p.name, "W25Q128JV");
        assert_eq!(p.vendor, "Winbond");
        assert_eq!(p.total_size, 16 * 1024 * 1024); // 128 Mbit = 16 MiB
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup(0xFFFFFF).is_none());
    }

    #[test]
    fn fallback_has_sane_defaults() {
        let p = fallback(0xABCDEF);
        assert_eq!(p.mid, 0xABCDEF);
        assert_eq!(p.total_size, 2 * 1024 * 1024);
        assert!(p.wp.is_none());
    }
}
