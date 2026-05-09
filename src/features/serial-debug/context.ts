// Nothing fancy; just a place to house cross-component helpers if needed later.
// The store itself is already global via Pinia, so we re-export for ergonomic imports.
export { useSerialDebugStore } from '@/stores/serial-debug';
export { usePortManagerStore } from '@/stores/port-manager';
