import { onMounted } from "vue";
import { APP_VERSION } from "@/config/app";
import {
  fetchLatestJson,
  isNewerVersion,
  UPDATE_SOURCES,
} from "@/features/settings/update-sources";
import { toastState } from "@/composables/toastState";
import { isTauriRuntime } from "@/runtime";
import { type AutoUpdateIntervalId, useSettingsStore } from "@/stores/settings";
import { getManualUpdateFlags } from "@/utils/install-type";
import { rLog } from "@/utils/log";

export const AUTO_UPDATE_STARTUP_DELAY_MS = 4000;

const AUTO_UPDATE_INTERVAL_MS: Record<AutoUpdateIntervalId, number> = {
  off: 0,
  "1h": 60 * 60 * 1000,
  "6h": 6 * 60 * 60 * 1000,
  "12h": 12 * 60 * 60 * 1000,
  "24h": 24 * 60 * 60 * 1000,
};

/** Detect current platform key for portable manifest lookup */
function getPortablePlatformKey(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("win")) return "windows-x86_64";
  if (ua.includes("mac")) {
    const isArm =
      navigator.userAgent.includes("ARM") ||
      (navigator as { userAgentData?: { platform?: string } }).userAgentData
        ?.platform === "macOS";
    return isArm ? "darwin-aarch64" : "darwin-x86_64";
  }
  return "linux-x86_64";
}

export function shouldAutoCheckForUpdate({
  isTauri,
  interval,
  lastCheckedAt,
  now,
}: {
  isTauri: boolean;
  interval: AutoUpdateIntervalId;
  lastCheckedAt: number | null;
  now: number;
}): boolean {
  if (!isTauri || interval === "off") return false;
  if (lastCheckedAt === null) return true;
  return now - lastCheckedAt >= AUTO_UPDATE_INTERVAL_MS[interval];
}

export function useAutoUpdate(): void {
  const settings = useSettingsStore();

  onMounted(() => {
    void settings.ready().then(async () => {
      const lastCheckedAt = await settings.getAutoUpdateLastCheckAt();
      if (
        !shouldAutoCheckForUpdate({
          isTauri: isTauriRuntime(),
          interval: settings.autoUpdateInterval,
          lastCheckedAt,
          now: Date.now(),
        })
      ) {
        return;
      }

      // Delay startup check by 4 seconds to not interfere with app startup
      setTimeout(() => {
        void checkForUpdate(settings);
      }, AUTO_UPDATE_STARTUP_DELAY_MS);
    });
  });
}

async function checkForUpdate(settings = useSettingsStore()): Promise<void> {
  rLog.info(`[Update] Auto-check starting (current: v${APP_VERSION})`);
  const flags = await getManualUpdateFlags();
  if (flags.manualOnly) {
    rLog.info(
      "[Update] Manual update install (portable or deb/rpm), toast will use releases-page flow",
    );
  }
  // Try sources in order (GitHub first, then Tuya OSS for mainland China).
  // If a source succeeds (whether update found or not), stop.
  // Only fall through to the next source on network/fetch failure.
  for (const source of UPDATE_SOURCES) {
    try {
      const manifest = await fetchLatestJson(source.url, 8000);
      await settings.setAutoUpdateLastCheckAt(Date.now());
      if (isNewerVersion(manifest.version, APP_VERSION)) {
        rLog.info(
          `[Update] New version found: v${manifest.version} (source: ${source.id})`,
        );
        toastState.version = manifest.version;
        toastState.isPortable = flags.manualOnly;
        if (flags.manualOnly) {
          if (flags.debRpm) {
            toastState.portableUrl = source.releasePageUrl;
          } else {
            const platformKey = getPortablePlatformKey();
            toastState.portableUrl =
              manifest.portable?.[platformKey]?.url || source.releasePageUrl;
          }
        }
        toastState.visible = true;
      } else {
        rLog.info(
          `[Update] Already up to date: v${APP_VERSION} (source: ${source.id})`,
        );
      }
      return; // Success (update found or already latest), stop checking
    } catch {
      rLog.warn(`[Update] Source '${source.id}' failed, trying next...`);
      // This source failed (timeout / network error), try next
    }
  }
  rLog.warn("[Update] All sources failed");
  // All sources failed — silent, don't disturb the user
}
