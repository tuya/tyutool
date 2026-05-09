import { defineStore } from 'pinia';
import { computed, ref } from 'vue';

export type PortOwnerId = 'flash' | 'serial-debug' | string;
export type ReleaseReason = 'requested' | 'unplugged' | 'error';

export interface PortClaim {
  id: PortOwnerId;
  port: string;
  /** Called when another owner wants this port. Return true to consent release. */
  onReleaseRequest: (requester: PortOwnerId) => Promise<boolean>;
  /** Called after the owner has been released. */
  onReleased: (reason: ReleaseReason) => void;
}

export const usePortManagerStore = defineStore('port-manager', () => {
  const owners = ref(new Map<string, PortClaim>());
  const resume = ref(new Map<string, PortOwnerId>());

  function currentOwner(port: string): PortOwnerId | null {
    return owners.value.get(port)?.id ?? null;
  }

  function resumeCandidate(port: string): PortOwnerId | null {
    return resume.value.get(port) ?? null;
  }

  function isOwnedBy(port: string, ownerId: PortOwnerId) {
    return computed(() => currentOwner(port) === ownerId);
  }

  async function acquire(claim: PortClaim): Promise<'ok' | 'denied'> {
    const current = owners.value.get(claim.port);
    if (!current) {
      owners.value.set(claim.port, claim);
      if (resume.value.get(claim.port) === claim.id) {
        resume.value.delete(claim.port);
      }
      return 'ok';
    }
    if (current.id === claim.id) {
      // Refresh callbacks; same owner.
      owners.value.set(claim.port, claim);
      return 'ok';
    }
    const approved = await current.onReleaseRequest(claim.id);
    if (!approved) {
      return 'denied';
    }
    try {
      current.onReleased('requested');
    } catch (e) {
      // Don't let a misbehaving callback block acquisition.
      console.warn('[port-manager] onReleased("requested") threw:', e);
    }
    owners.value.set(claim.port, claim);
    if (resume.value.get(claim.port) === claim.id) {
      resume.value.delete(claim.port);
    }
    return 'ok';
  }

  function release(port: string, ownerId: PortOwnerId): void {
    const current = owners.value.get(port);
    if (!current || current.id !== ownerId) {
      return;
    }
    owners.value.delete(port);
  }

  function registerResume(port: string, ownerId: PortOwnerId): void {
    resume.value.set(port, ownerId);
  }

  function notifyUnplugged(port: string): void {
    const current = owners.value.get(port);
    if (current) {
      try {
        current.onReleased('unplugged');
      } catch (e) {
        console.warn('[port-manager] onReleased("unplugged") threw:', e);
      }
      owners.value.delete(port);
    }
    resume.value.delete(port);
  }

  return {
    acquire,
    release,
    registerResume,
    notifyUnplugged,
    currentOwner,
    resumeCandidate,
    isOwnedBy,
  };
});
