//! ESP chip definitions — static parameters per chip variant.

use espflash::target::Chip;

/// Static definition of one ESP chip variant for the flash plugin.
pub(crate) struct EspChipDef {
    /// Registry id, uppercase (e.g. `ESP32`, `ESP32C3`).
    pub id: &'static str,
    /// The espflash chip enum variant.
    pub chip: Chip,
}

pub(crate) static ESP32_DEF: EspChipDef = EspChipDef {
    id: "ESP32",
    chip: Chip::Esp32,
};

pub(crate) static ESP32C3_DEF: EspChipDef = EspChipDef {
    id: "ESP32C3",
    chip: Chip::Esp32c3,
};

pub(crate) static ESP32C6_DEF: EspChipDef = EspChipDef {
    id: "ESP32C6",
    chip: Chip::Esp32c6,
};

pub(crate) static ESP32S3_DEF: EspChipDef = EspChipDef {
    id: "ESP32S3",
    chip: Chip::Esp32s3,
};
