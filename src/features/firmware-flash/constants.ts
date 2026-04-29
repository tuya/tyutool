/** Supported chip identifiers (UI list sorted by ASCII string order). */
export const CHIP_IDS = ['bk7231n', 'esp32', 'esp32c3', 'esp32c6', 'esp32s3', 't1', 't2', 't3', 't5'] as const;

export type ChipId = (typeof CHIP_IDS)[number];

/** Default chip when no saved workspace exists (first launch). */
export const DEFAULT_CHIP_ID: ChipId = 't5';

/** Initially empty; populated at runtime by Tauri serial port scan. */
export const SERIAL_PORT_OPTIONS: Array<{ value: string; label: string }> = [];

export const BAUD_RATE_OPTIONS = [115200, 460800, 921600, 1000000, 1500000, 2000000] as const;

/** Default baud rate for TuyaOpen UART authorization (independent of flash baud). */
export const AUTH_BAUD_RATE_DEFAULT = 115200 as const;
