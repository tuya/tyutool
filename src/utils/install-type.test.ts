import { describe, expect, it } from 'vitest';
import { canUseInAppUpdater, getManualUpdateFlagsForInstallType } from './install-type';

describe('getManualUpdateFlagsForInstallType', () => {
  it('requires manual updates for portable installs', () => {
    expect(getManualUpdateFlagsForInstallType('portable (/tmp/tyutool)')).toEqual({
      manualOnly: true,
      debRpm: false,
    });
  });

  it('requires manual updates for deb/rpm installs', () => {
    expect(getManualUpdateFlagsForInstallType('deb/rpm (installed)')).toEqual({
      manualOnly: true,
      debRpm: true,
    });
  });

  it('allows in-app updates for AppImage installs', () => {
    expect(getManualUpdateFlagsForInstallType('AppImage')).toEqual({
      manualOnly: false,
      debRpm: false,
    });
  });

  it('does not assume manual-only behavior for unknown installs', () => {
    expect(getManualUpdateFlagsForInstallType('unknown')).toEqual({
      manualOnly: false,
      debRpm: false,
    });
  });
});

describe('canUseInAppUpdater', () => {
  it('waits for install type detection before enabling the in-app updater', () => {
    expect(canUseInAppUpdater(false, { manualOnly: false, debRpm: false })).toBe(false);
  });

  it('disables the in-app updater for manual-only installs', () => {
    expect(canUseInAppUpdater(true, { manualOnly: true, debRpm: true })).toBe(false);
  });

  it('enables the in-app updater after supported install type detection', () => {
    expect(canUseInAppUpdater(true, { manualOnly: false, debRpm: false })).toBe(true);
  });
});
