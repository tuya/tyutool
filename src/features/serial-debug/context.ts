// Nothing fancy; just a place to house cross-component helpers if needed later.
// The store itself is already global via Pinia, so we re-export for ergonomic imports.
export { useSerialDebugStore } from '@/stores/serial-debug';
export { usePortManagerStore } from '@/stores/port-manager';

/**
 * Converts a serial port name to a safe directory name.
 * Strips leading slash (Unix paths) and replaces filesystem-unsafe chars with _.
 */
export function sanitizePortName(port: string): string {
  const stripped = port.startsWith('/') ? port.slice(1) : port;
  return stripped.replace(/[/\\:*?"<>|.]/g, '_');
}

export function makeStamp(): string {
  const now = new Date();
  return `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}-${String(now.getHours()).padStart(2, '0')}${String(now.getMinutes()).padStart(2, '0')}${String(now.getSeconds()).padStart(2, '0')}`;
}

export function formatTs(ms: number): string {
  const d = new Date(ms);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  const ss = String(d.getSeconds()).padStart(2, '0');
  const mmm = String(d.getMilliseconds()).padStart(3, '0');
  return `${hh}:${mm}:${ss}.${mmm}`;
}
