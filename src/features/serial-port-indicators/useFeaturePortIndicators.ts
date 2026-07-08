import { computed } from "vue";
import { useBatchFlashAuthStore } from "@/stores/batch-flash-auth";
import { useFlashStore } from "@/stores/flash";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { useSettingsStore, type ThemePreference } from "@/stores/settings";
import {
  buildFeaturePortIndicators,
  type FeatureIndicatorName,
  type IndicatorPaletteMode,
} from "./model";

export function resolveIndicatorPaletteMode(
  theme: ThemePreference,
): IndicatorPaletteMode {
  if (theme === "dark") return "dark";
  if (theme === "light") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function useFeaturePortIndicators() {
  const settings = useSettingsStore();
  const flash = useFlashStore();
  const serialDebug = useSerialDebugStore();
  const batchFlashAuth = useBatchFlashAuthStore();

  const indicators = computed(() =>
    buildFeaturePortIndicators({
      enabled: settings.serialPortIndicatorsEnabled,
      flash: {
        connected: flash.connected,
        port: flash.selectedSerialPort,
      },
      serialDebug: {
        open: serialDebug.open,
        port: serialDebug.port,
      },
      toolboxSlots: batchFlashAuth.slots.map((slot) => ({
        port: slot.port,
        status: slot.status,
      })),
    }),
  );

  const activePorts = computed(() => [
    ...new Set(
      Object.values(indicators.value).flatMap((indicator) => indicator.ports),
    ),
  ]);

  const paletteMode = computed(() =>
    resolveIndicatorPaletteMode(settings.theme),
  );

  function indicatorForFeature(feature: FeatureIndicatorName) {
    return indicators.value[feature];
  }

  return {
    indicators,
    activePorts,
    paletteMode,
    indicatorForFeature,
  };
}
