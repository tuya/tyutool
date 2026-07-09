import { defineStore } from "pinia";
import { ref } from "vue";
import { rLog } from "@/utils/log";

export type PortOwnerId = "flash" | "serial-debug" | string;
export type ReleaseReason = "requested" | "unplugged" | "error";

export interface PortClaim {
  id: PortOwnerId;
  port: string;
  /** Called when another owner wants this port. Return true to consent release. */
  onReleaseRequest: (requester: PortOwnerId) => Promise<boolean>;
  /** Called after the owner has been released. */
  onReleased: (reason: ReleaseReason) => void | Promise<void>;
}

export const usePortManagerStore = defineStore("port-manager", () => {
  const owners = ref(new Map<string, PortClaim>());
  const resume = ref(new Map<string, PortOwnerId>());
  const releasing = ref(new Map<string, PortClaim>());

  function currentOwner(port: string): PortOwnerId | null {
    return owners.value.get(port)?.id ?? null;
  }

  function resumeCandidate(port: string): PortOwnerId | null {
    return resume.value.get(port) ?? null;
  }

  async function acquire(claim: PortClaim): Promise<"ok" | "denied"> {
    const current = owners.value.get(claim.port);
    if (!current) {
      owners.value.set(claim.port, claim);
      if (resume.value.get(claim.port) === claim.id) {
        resume.value.delete(claim.port);
      }
      rLog.info(
        `[port-manager] acquired port=${claim.port} by=${claim.id} (was free)`,
      );
      return "ok";
    }
    if (current.id === claim.id) {
      // Refresh callbacks; same owner.
      owners.value.set(claim.port, claim);
      rLog.debug(
        `[port-manager] acquired port=${claim.port} by=${claim.id} (refresh)`,
      );
      return "ok";
    }
    const approved = await current.onReleaseRequest(claim.id);
    if (!approved) {
      rLog.info(
        `[port-manager] acquire denied port=${claim.port} by=${claim.id} (owner ${current.id} refused)`,
      );
      return "denied";
    }
    // Re-check: another concurrent acquire may have already swapped the owner
    // while we were awaiting this onReleaseRequest (e.g. two dialogs in flight).
    const stillCurrent = owners.value.get(claim.port);
    if (!stillCurrent || stillCurrent !== current) {
      // Owner changed during the await. Don't forcibly steal from a newcomer —
      // treat as if we lost the race and let the caller retry if they want.
      rLog.info(
        `[port-manager] acquire denied port=${claim.port} by=${claim.id} (owner changed during await)`,
      );
      return "denied";
    }
    if (releasing.value.get(claim.port) === current) {
      rLog.info(
        `[port-manager] acquire denied port=${claim.port} by=${claim.id} (owner already releasing)`,
      );
      return "denied";
    }
    releasing.value.set(claim.port, current);
    try {
      await current.onReleased("requested");
    } catch (e) {
      rLog.warn(`[port-manager] onReleased("requested") threw: ${e}`);
    } finally {
      if (releasing.value.get(claim.port) === current) {
        releasing.value.delete(claim.port);
      }
    }
    const ownerAfterRelease = owners.value.get(claim.port);
    if (!ownerAfterRelease || ownerAfterRelease !== current) {
      rLog.info(
        `[port-manager] acquire denied port=${claim.port} by=${claim.id} (owner changed after release)`,
      );
      return "denied";
    }
    owners.value.set(claim.port, claim);
    if (resume.value.get(claim.port) === claim.id) {
      resume.value.delete(claim.port);
    }
    rLog.info(
      `[port-manager] acquired port=${claim.port} by=${claim.id} (preempted ${current.id})`,
    );
    return "ok";
  }

  /**
   * Voluntary release by the current owner. Does NOT fire `onReleased` because
   * the releaser is, by definition, the thing that already knows it's releasing.
   * `onReleased` is for *involuntary* transitions (preemption via another `acquire`
   * or a hotplug via `notifyUnplugged`). A mismatched owner id is a no-op.
   */
  function release(port: string, ownerId: PortOwnerId): void {
    const current = owners.value.get(port);
    if (!current || current.id !== ownerId) {
      return;
    }
    rLog.info(`[port-manager] released port=${port} by=${ownerId}`);
    owners.value.delete(port);
  }

  function registerResume(port: string, ownerId: PortOwnerId): void {
    resume.value.set(port, ownerId);
  }

  function notifyUnplugged(port: string): void {
    const current = owners.value.get(port);
    if (current) {
      rLog.info(`[port-manager] unplugged port=${port} (was ${current.id})`);
      try {
        current.onReleased("unplugged");
      } catch (e) {
        rLog.warn(`[port-manager] onReleased("unplugged") threw: ${e}`);
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
  };
});
