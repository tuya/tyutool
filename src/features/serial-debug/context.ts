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
