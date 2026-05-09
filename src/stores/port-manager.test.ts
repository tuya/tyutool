import { setActivePinia, createPinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { usePortManagerStore, type PortClaim } from './port-manager';

function makeClaim(id: string, port: string, overrides: Partial<PortClaim> = {}): PortClaim {
  return {
    id,
    port,
    onReleaseRequest: async () => true,
    onReleased: () => {},
    ...overrides,
  };
}

describe('port-manager', () => {
  beforeEach(() => setActivePinia(createPinia()));

  it('acquires a free port', async () => {
    const pm = usePortManagerStore();
    const r = await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    expect(r).toBe('ok');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBe('flash');
  });

  it('is idempotent for the same owner', async () => {
    const pm = usePortManagerStore();
    await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    const r = await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    expect(r).toBe('ok');
  });

  it('asks the current owner to release and respects approval', async () => {
    const pm = usePortManagerStore();
    const onReleaseRequest = vi.fn(async () => true);
    const onReleased = vi.fn();
    await pm.acquire(makeClaim('serial-debug', '/dev/ttyUSB0', { onReleaseRequest, onReleased }));
    const r = await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    expect(r).toBe('ok');
    expect(onReleaseRequest).toHaveBeenCalledWith('flash');
    expect(onReleased).toHaveBeenCalledWith('requested');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBe('flash');
  });

  it('respects denial from the current owner', async () => {
    const pm = usePortManagerStore();
    await pm.acquire(makeClaim('serial-debug', '/dev/ttyUSB0', { onReleaseRequest: async () => false }));
    const r = await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    expect(r).toBe('denied');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBe('serial-debug');
  });

  it('release clears the owner', async () => {
    const pm = usePortManagerStore();
    await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    pm.release('/dev/ttyUSB0', 'flash');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBeNull();
  });

  it('release is a no-op when owner id does not match', async () => {
    const pm = usePortManagerStore();
    await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    pm.release('/dev/ttyUSB0', 'serial-debug');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBe('flash');
  });

  it('notifyUnplugged clears the owner and calls onReleased with "unplugged"', async () => {
    const pm = usePortManagerStore();
    const onReleased = vi.fn();
    await pm.acquire(makeClaim('serial-debug', '/dev/ttyUSB0', { onReleased }));
    pm.notifyUnplugged('/dev/ttyUSB0');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBeNull();
    expect(onReleased).toHaveBeenCalledWith('unplugged');
  });

  it('registerResume + release notifies the resumer', async () => {
    const pm = usePortManagerStore();
    await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    pm.registerResume('/dev/ttyUSB0', 'serial-debug');
    pm.release('/dev/ttyUSB0', 'flash');
    expect(pm.currentOwner('/dev/ttyUSB0')).toBeNull();
    expect(pm.resumeCandidate('/dev/ttyUSB0')).toBe('serial-debug');
  });

  it('clears resume candidate after it claims', async () => {
    const pm = usePortManagerStore();
    await pm.acquire(makeClaim('flash', '/dev/ttyUSB0'));
    pm.registerResume('/dev/ttyUSB0', 'serial-debug');
    pm.release('/dev/ttyUSB0', 'flash');
    await pm.acquire(makeClaim('serial-debug', '/dev/ttyUSB0'));
    expect(pm.resumeCandidate('/dev/ttyUSB0')).toBeNull();
  });

  it('when two concurrent acquires pass onReleaseRequest, only the first wins', async () => {
    const pm = usePortManagerStore();
    let resolveDialog1: (v: boolean) => void;
    let resolveDialog2: (v: boolean) => void;
    const onReleaseRequest = vi.fn((requester: string) =>
      requester === 'A'
        ? new Promise<boolean>((r) => { resolveDialog1 = r; })
        : new Promise<boolean>((r) => { resolveDialog2 = r; }),
    );
    await pm.acquire(makeClaim('current', '/dev/ttyUSB0', { onReleaseRequest }));

    // Two acquires race while both dialogs are pending.
    const a = pm.acquire(makeClaim('A', '/dev/ttyUSB0'));
    const b = pm.acquire(makeClaim('B', '/dev/ttyUSB0'));
    resolveDialog1!(true);
    resolveDialog2!(true);
    const [ra, rb] = await Promise.all([a, b]);

    // Exactly one succeeds; the other must be denied.
    const wins = [ra, rb].filter((r) => r === 'ok').length;
    expect(wins).toBe(1);
    expect(['A', 'B']).toContain(pm.currentOwner('/dev/ttyUSB0'));
  });
});
