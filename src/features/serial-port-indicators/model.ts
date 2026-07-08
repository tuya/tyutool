export type FeatureIndicatorName = "flash" | "serial-debug" | "toolbox";
export type FeaturePortIndicatorDisplayMode = "single-port" | "count";
export type IndicatorPaletteMode = "light" | "dark";

export type FeaturePortIndicator = {
  enabled: boolean;
  active: boolean;
  ports: string[];
  count: number;
  displayMode: FeaturePortIndicatorDisplayMode;
};

export type ToolboxIndicatorSlot = {
  port: string;
  status: string;
};

export type FeaturePortIndicatorMap = Record<
  FeatureIndicatorName,
  FeaturePortIndicator
>;

export const ACTIVE_TOOLBOX_SLOT_STATUSES = [
  "reading",
  "flashing",
  "reading_mac",
  "authorizing",
] as const;

const LIGHT_PORT_COLORS = [
  "#1d4ed8",
  "#0f766e",
  "#be185d",
  "#7c3aed",
  "#b45309",
  "#047857",
  "#c2410c",
  "#0369a1",
  "#6d28d9",
  "#15803d",
  "#b91c1c",
  "#a21caf",
] as const;

const DARK_PORT_COLORS = [
  "#60a5fa",
  "#34d399",
  "#f472b6",
  "#a78bfa",
  "#fbbf24",
  "#2dd4bf",
  "#fb923c",
  "#38bdf8",
  "#c084fc",
  "#4ade80",
  "#f87171",
  "#5eead4",
] as const;

function buildIndicator(
  enabled: boolean,
  ports: string[],
  displayMode: FeaturePortIndicatorDisplayMode,
): FeaturePortIndicator {
  if (!enabled) {
    return {
      enabled: false,
      active: false,
      ports: [],
      count: 0,
      displayMode,
    };
  }

  return {
    enabled: true,
    active: ports.length > 0,
    ports,
    count: ports.length,
    displayMode,
  };
}

function normalizePorts(ports: readonly string[]): string[] {
  return [...new Set(ports.filter((port) => port.trim().length > 0))].sort(
    (left, right) =>
      left.localeCompare(right, undefined, {
        numeric: true,
        sensitivity: "base",
      }),
  );
}

export function buildFeaturePortIndicators(input: {
  enabled: boolean;
  flash: { connected: boolean; port: string };
  serialDebug: { open: boolean; port: string };
  toolboxSlots: ToolboxIndicatorSlot[];
}): FeaturePortIndicatorMap {
  const flashPorts =
    input.flash.connected && input.flash.port.trim()
      ? [input.flash.port.trim()]
      : [];
  const serialDebugPorts =
    input.serialDebug.open && input.serialDebug.port.trim()
      ? [input.serialDebug.port.trim()]
      : [];
  const toolboxPorts = normalizePorts(
    input.toolboxSlots
      .filter((slot) =>
        (ACTIVE_TOOLBOX_SLOT_STATUSES as readonly string[]).includes(
          slot.status,
        ),
      )
      .map((slot) => slot.port.trim()),
  );

  return {
    flash: buildIndicator(input.enabled, flashPorts, "single-port"),
    "serial-debug": buildIndicator(
      input.enabled,
      serialDebugPorts,
      "single-port",
    ),
    toolbox: buildIndicator(input.enabled, toolboxPorts, "count"),
  };
}

export function resolvePortIndicatorColor(
  port: string,
  activePorts: readonly string[],
  paletteMode: IndicatorPaletteMode,
): string {
  const normalizedPort = port.trim();
  const orderedPorts = normalizePorts([...activePorts, normalizedPort]);
  const palette = paletteMode === "dark" ? DARK_PORT_COLORS : LIGHT_PORT_COLORS;
  const index = Math.max(0, orderedPorts.indexOf(normalizedPort));
  return palette[index % palette.length];
}
