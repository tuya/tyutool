import { describe, expect, it } from 'vitest';
import { CHIP_MANIFEST, chipManifest, rustPluginIdForChip } from './chip-manifests';
import { CHIP_IDS } from './constants';

describe('CHIP_MANIFEST', () => {
  it('has an entry for every CHIP_ID', () => {
    for (const id of CHIP_IDS) {
      expect(CHIP_MANIFEST).toHaveProperty(id);
    }
  });

  it('every manifest has required fields', () => {
    for (const id of CHIP_IDS) {
      const m = CHIP_MANIFEST[id];
      expect(m.rustPluginId).toEqual(expect.any(String));
      expect(m.defaultBaudRate).toEqual(expect.any(Number));
      expect(m.defaultBaudRate).toBeGreaterThan(0);
      expect(m.flashSize).toMatch(/^0x[0-9a-fA-F]+$/);
      expect(m.eraseRequires4KAlignment).toBe(true);
      // Every chip must have at least one erase preset
      expect(Object.keys(m.erasePresets).length).toBeGreaterThan(0);
    }
  });

  it('ESP chips have fullChip preset', () => {
    for (const id of CHIP_IDS) {
      if (!id.startsWith('esp')) continue;
      const m = CHIP_MANIFEST[id];
      expect(m.erasePresets.fullChip).toBeDefined();
      expect(m.erasePresets.authInfo).toBeUndefined();
      expect(m.erasePresets.fullChipNoRf).toBeUndefined();
    }
  });

  it('Beken chips have authInfo and fullChipNoRf presets', () => {
    const bekenIds = CHIP_IDS.filter(id => !id.startsWith('esp'));
    for (const id of bekenIds) {
      const m = CHIP_MANIFEST[id];
      expect(m.erasePresets.authInfo).toBeDefined();
      expect(m.erasePresets.fullChipNoRf).toBeDefined();
    }
  });

  it('erase presets have valid hex addresses', () => {
    for (const id of CHIP_IDS) {
      const m = CHIP_MANIFEST[id];
      for (const preset of Object.values(m.erasePresets)) {
        if (!preset) continue;
        expect(preset.start).toMatch(/^0x[0-9a-fA-F]+$/);
        expect(preset.end).toMatch(/^0x[0-9a-fA-F]+$/);
      }
    }
  });
});

describe('chipManifest', () => {
  it('returns manifest for known chip IDs', () => {
    const m = chipManifest('t5');
    expect(m.rustPluginId).toBe('T5');
    expect(m.defaultBaudRate).toBe(921600);
  });

  it('returns manifest for bk7231n', () => {
    const m = chipManifest('bk7231n');
    expect(m.rustPluginId).toBe('BK7231N');
  });

  it('returns manifest for ESP32 chips', () => {
    expect(chipManifest('esp32').rustPluginId).toBe('ESP32');
    expect(chipManifest('esp32c3').rustPluginId).toBe('ESP32C3');
    expect(chipManifest('esp32c6').rustPluginId).toBe('ESP32C6');
    expect(chipManifest('esp32s3').rustPluginId).toBe('ESP32S3');
  });

  it('ESP chips have 460800 default baud', () => {
    for (const id of ['esp32', 'esp32c3', 'esp32c6', 'esp32s3'] as const) {
      expect(chipManifest(id).defaultBaudRate).toBe(460800);
    }
  });

  it('throws for unknown chip ID', () => {
    expect(() => chipManifest('unknown_chip')).toThrow('Unknown chip: unknown_chip');
  });
});

describe('rustPluginIdForChip', () => {
  it('maps known UI IDs to Rust plugin IDs', () => {
    expect(rustPluginIdForChip('t5')).toBe('T5');
    expect(rustPluginIdForChip('t1')).toBe('T1');
    expect(rustPluginIdForChip('t2')).toBe('T2');
    expect(rustPluginIdForChip('bk7231n')).toBe('BK7231N');
    expect(rustPluginIdForChip('esp32')).toBe('ESP32');
    expect(rustPluginIdForChip('esp32c3')).toBe('ESP32C3');
    expect(rustPluginIdForChip('esp32s3')).toBe('ESP32S3');
  });

  it('falls back to uppercase for unknown IDs', () => {
    expect(rustPluginIdForChip('new-chip')).toBe('NEWCHIP');
    expect(rustPluginIdForChip('abc')).toBe('ABC');
  });
});
