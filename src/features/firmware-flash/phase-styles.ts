export interface PhaseStyle {
  /** CSS background gradient, used directly in inline style */
  gradient: string
  /** vue-i18n key for displaying the phase name */
  labelKey: string
}

// FlashPhase serde serialization (rename_all = "snake_case"):
//   unit variant  → string: "erase", "write", "verify", "read"
//   struct variant → object: {"write_segment": {"current":1,"total":1}}
// Phases NOT in this table (write_segment, handshake, etc.) do not update the progress bar.
export const PHASE_STYLES: Record<string, PhaseStyle> = {
  erase: {
    gradient:
      'linear-gradient(90deg, var(--ty-accent), color-mix(in srgb, var(--ty-accent) 70%, var(--ty-primary)))',
    labelKey: 'flash.phaseErase',
  },
  write: {
    gradient:
      'linear-gradient(90deg, var(--ty-primary), color-mix(in srgb, var(--ty-primary) 70%, var(--ty-accent)))',
    labelKey: 'flash.phaseWrite',
  },
  verify: {
    gradient:
      'linear-gradient(90deg, var(--ty-success), color-mix(in srgb, var(--ty-success) 80%, var(--ty-primary)))',
    labelKey: 'flash.phaseVerify',
  },
  read: {
    gradient:
      'linear-gradient(90deg, var(--ty-primary), color-mix(in srgb, var(--ty-primary) 60%, var(--ty-success)))',
    labelKey: 'flash.phaseRead',
  },
}

/** Extract PHASE_STYLES lookup key from a raw FlashEvent phase value */
export function phaseKey(phase: unknown): string {
  if (typeof phase === 'string') return phase
  if (typeof phase === 'object' && phase !== null) {
    return Object.keys(phase)[0] ?? 'unknown'
  }
  return 'unknown'
}
