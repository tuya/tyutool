import { describe, expect, it, vi } from 'vitest';
import {
  coerceUsbU16,
  formatSerialPortLabel,
  tuyaDualSerialHoverTooltip,
  TUYA_DUAL_SERIAL_PROBE_PID,
  TUYA_DUAL_SERIAL_PROBE_VID,
  type TauriSerialPortRow,
} from './serial-port-label';

describe('formatSerialPortLabel', () => {
  const t = vi.fn((key: string) => {
    if (key === 'flash.portRoles.flash_auth') {
      return '烧录/授权';
    }
    if (key === 'flash.portRoles.log') {
      return '日志';
    }
    return key;
  });

  it('uses path only when no name and no role', () => {
    const p: TauriSerialPortRow = { path: '/dev/ttyUSB0' };
    expect(formatSerialPortLabel(p, t)).toBe('/dev/ttyUSB0');
  });

  it('includes name when present', () => {
    const p: TauriSerialPortRow = { path: '/dev/ttyACM0', name: 'USB Serial' };
    expect(formatSerialPortLabel(p, t)).toBe('/dev/ttyACM0 (USB Serial)');
  });

  it('appends translated role when portRole is known', () => {
    const p: TauriSerialPortRow = {
      path: '/dev/ttyACM0',
      name: 'Dual',
      portRole: 'flash_auth',
    };
    expect(formatSerialPortLabel(p, t)).toBe('/dev/ttyACM0 (Dual) — 烧录/授权');
  });

  it('ignores unknown portRole keys', () => {
    const p: TauriSerialPortRow = { path: '/dev/ttyUSB1', portRole: 'unknown_role' };
    expect(formatSerialPortLabel(p, t)).toBe('/dev/ttyUSB1');
  });
});

describe('coerceUsbU16', () => {
  it('parses decimal and hex strings', () => {
    expect(coerceUsbU16('6790')).toBe(0x1a86);
    expect(coerceUsbU16('0x1a86')).toBe(0x1a86);
    expect(coerceUsbU16(0x1a86)).toBe(0x1a86);
  });
});

describe('tuyaDualSerialHoverTooltip', () => {
  const t = (key: string) => {
    if (key === 'flash.tuyaPortHint.maybeFlashAuth') {
      return '可能是烧录授权串口';
    }
    if (key === 'flash.tuyaPortHint.maybeLog') {
      return '可能是日志串口';
    }
    if (key === 'flash.tuyaPortHint.interfaceUnknown') {
      return '接口未知提示';
    }
    return key;
  };

  it('returns null for other VID/PID', () => {
    expect(tuyaDualSerialHoverTooltip(0x10c4, 0xea60, 0, t)).toBeNull();
  });

  it('returns null when interface is outside known dual-serial pairs', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, 4, t)).toBeNull();
  });

  it('accepts string VID/PID from IPC', () => {
    expect(tuyaDualSerialHoverTooltip('6790', '21970', undefined, t)).toBe('接口未知提示');
  });

  it('returns flash hint for interface 0', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, 0, t)).toBe(
      '可能是烧录授权串口'
    );
  });

  it('returns flash hint for macOS data interface 1', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, 1, t)).toBe(
      '可能是烧录授权串口'
    );
  });

  it('returns flash hint when interface is string "0"', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, '0', t)).toBe(
      '可能是烧录授权串口'
    );
  });

  it('returns log hint for interface 2', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, 2, t)).toBe(
      '可能是日志串口'
    );
  });

  it('returns log hint for macOS data interface 3', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, 3, t)).toBe(
      '可能是日志串口'
    );
  });

  it('returns generic hint when interface is missing (e.g. Linux)', () => {
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, undefined, t)).toBe(
      '接口未知提示'
    );
    expect(tuyaDualSerialHoverTooltip(TUYA_DUAL_SERIAL_PROBE_VID, TUYA_DUAL_SERIAL_PROBE_PID, null, t)).toBe(
      '接口未知提示'
    );
  });
});
