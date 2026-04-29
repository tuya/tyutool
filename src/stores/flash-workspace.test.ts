import { describe, expect, it } from 'vitest';
import { parseFlashWorkspaceJson, WORKSPACE_VERSION } from './flash-workspace';

describe('parseFlashWorkspaceJson', () => {
  it('returns null for empty or invalid input', () => {
    expect(parseFlashWorkspaceJson(null)).toBeNull();
    expect(parseFlashWorkspaceJson('')).toBeNull();
    expect(parseFlashWorkspaceJson('not json')).toBeNull();
    expect(parseFlashWorkspaceJson('{}')).toBeNull();
  });

  it('parses a minimal valid workspace', () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: 'flash',
      selectedSerialPort: '/dev/ttyUSB0',
      selectedBaudRate: 921600,
      selectedChipId: 't5',
      flashSegments: [
        {
          id: 'seg1',
          firmwarePath: '/tmp/a.bin',
          startAddr: '0x00000000',
          endAddr: '0x00001000',
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: '0x00000000',
      eraseEndAddr: '0x00100000',
      readStartAddr: '0x00000000',
      readEndAddr: '0x00800000',
      readDir: '/home/u/out',
      readFileName: 'dump.bin',
      readFileNameModified: true,
      authorizeUuid: '',
      authorizeAuthKey: '',
      authBaudRate: 115200,
    });
    const w = parseFlashWorkspaceJson(raw);
    expect(w).not.toBeNull();
    expect(w!.selectedChipId).toBe('t5');
    expect(w!.flashSegments).toHaveLength(1);
    expect(w!.flashSegments[0].firmwarePath).toBe('/tmp/a.bin');
    expect(w!.readFileNameModified).toBe(true);
  });

  it('preserves authorize tab for ESP chips (UART-only flow)', () => {
    const raw = JSON.stringify({
      v: WORKSPACE_VERSION,
      activeTab: 'authorize',
      selectedSerialPort: '',
      selectedBaudRate: 115200,
      selectedChipId: 'esp32',
      flashSegments: [
        {
          id: 'a',
          firmwarePath: '',
          startAddr: '0x00000000',
          endAddr: '0x00000000',
        },
      ],
      activeSegmentIndex: 0,
      eraseAdvancedOpen: false,
      eraseStartAddr: '0x0',
      eraseEndAddr: '0x0',
      readStartAddr: '0x0',
      readEndAddr: '0x0',
      readDir: '',
      readFileName: 'x.bin',
      readFileNameModified: false,
      authorizeUuid: '',
      authorizeAuthKey: '',
      authBaudRate: 115200,
    });
    const w = parseFlashWorkspaceJson(raw);
    expect(w!.activeTab).toBe('authorize');
  });
});
