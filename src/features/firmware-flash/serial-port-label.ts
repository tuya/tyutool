/** Shape stored in flash Pinia for the serial `TySelect` (subset of `TySelectOption`). */
export type SerialPortDropdownOption = {
  value: string;
  label: string;
  disabled?: boolean;
  optionTooltip?: string;
};

/** Matches Windows hardware id `USB\\VID_1A86&PID_55D2` — common Tuya dual-USB-serial probe. */
export const TUYA_DUAL_SERIAL_PROBE_VID = 0x1a86;
export const TUYA_DUAL_SERIAL_PROBE_PID = 0x55d2;

/**
 * Coerce VID/PID from IPC/JSON (sometimes strings) to a 16-bit value for comparisons.
 */
export function coerceUsbU16(value: number | string | null | undefined): number | null {
  if (value == null) {
    return null;
  }
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value & 0xffff : null;
  }
  if (typeof value === 'string') {
    const s = value.trim();
    if (s === '') {
      return null;
    }
    const n = /^0x/i.test(s) ? Number.parseInt(s, 16) : Number(s);
    return Number.isFinite(n) ? n & 0xffff : null;
  }
  return null;
}

function coerceUsbInterface(value: number | string | null | undefined): number | null | undefined {
  if (value == null) {
    return value as null | undefined;
  }
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : undefined;
  }
  if (typeof value === 'string') {
    const s = value.trim();
    if (s === '') {
      return undefined;
    }
    const n = Number(s);
    return Number.isFinite(n) ? n : undefined;
  }
  return undefined;
}

/** One row from Tauri `list_serial_ports_cmd` (`SerialPortEntry`, serde camelCase). */
export type TauriSerialPortRow = {
  path: string;
  name?: string | null;
  usbVid?: number | null;
  usbPid?: number | null;
  usbSerial?: string | null;
  usbInterface?: number | null;
  portRole?: string | null;
};

/**
 * Builds the dropdown / log label: `path (product)` plus optional i18n role suffix
 * when `portRole` matches `flash.portRoles.<role>`.
 */
/**
 * Hover hint for VID/PID `1a86:55d2` only, keyed by USB interface (see `tmp/usb-port-survey.md`).
 * When the OS omits `usbInterface` (common on Linux), returns a generic dual-port hint instead of no tooltip.
 * macOS exposes CDC data interfaces (`1` / `3`), while Windows reports the paired control interfaces (`0` / `2`).
 * Returns `null` when not this probe or interface is outside those known dual-serial pairs.
 */
export function tuyaDualSerialHoverTooltip(
  usbVid: number | string | null | undefined,
  usbPid: number | string | null | undefined,
  usbInterface: number | string | null | undefined,
  t: (key: string) => string
): string | null {
  const vid = coerceUsbU16(usbVid);
  const pid = coerceUsbU16(usbPid);
  if (vid !== TUYA_DUAL_SERIAL_PROBE_VID || pid !== TUYA_DUAL_SERIAL_PROBE_PID) {
    return null;
  }
  const iface = coerceUsbInterface(usbInterface);
  if (iface === 0 || iface === 1) {
    return t('flash.tuyaPortHint.maybeFlashAuth');
  }
  if (iface === 2 || iface === 3) {
    return t('flash.tuyaPortHint.maybeLog');
  }
  if (iface == null) {
    return t('flash.tuyaPortHint.interfaceUnknown');
  }
  return null;
}

export function formatSerialPortLabel(p: TauriSerialPortRow, t: (key: string) => string): string {
  const base = p.name ? `${p.path} (${p.name})` : p.path;
  const role = p.portRole?.trim();
  if (!role) {
    return base;
  }
  const key = `flash.portRoles.${role}`;
  const translated = t(key);
  if (translated === key) {
    return base;
  }
  return `${base} — ${translated}`;
}
