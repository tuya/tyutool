// src/features/batch-flash-auth/port-filter.ts

function isWindowsPlatform(): boolean {
  if (typeof navigator !== "undefined" && navigator.platform) {
    return navigator.platform.toLowerCase().includes("win");
  }
  if (typeof process !== "undefined") {
    return process.platform === "win32";
  }
  return false;
}

/** On Windows, COM port names are case-insensitive (COM3 = com3). Unix-style paths are unchanged. */
export function normalizePortName(port: string): string {
  const trimmed = port.trim();
  if (isWindowsPlatform() && /^COM\d+$/i.test(trimmed)) {
    return trimmed.toUpperCase();
  }
  return port;
}

/** Remove ports that match any entry in blockedPorts (case-insensitive for COM ports on Windows). */
export function applyPortFilter(
  ports: string[],
  blockedPorts: string[],
): string[] {
  if (blockedPorts.length === 0) return ports;
  const blocked = new Set(blockedPorts.map(normalizePortName));
  return ports.filter((p) => !blocked.has(normalizePortName(p)));
}
