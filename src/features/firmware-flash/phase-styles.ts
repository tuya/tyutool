export interface PhaseStyle {
  /** CSS background gradient, used directly in inline style */
  gradient: string
  /** CSS color value for the glow box-shadow (a CSS variable reference) */
  glowColor: string
  /** vue-i18n key for displaying the phase name */
  labelKey: string
}

// FlashPhase serde serialization (rename_all = "snake_case"):
//   unit variant  → string: "erase", "write", "verify", "read"
//   struct variant → object: {"write_segment": {"current":1,"total":1}}
// Phases NOT in this table (write_segment, handshake, etc.) do not update the progress bar.
export const PHASE_STYLES: Record<string, PhaseStyle> = {
  erase: {
    gradient:  'linear-gradient(90deg, var(--phase-erase-from), var(--phase-erase-to))',
    glowColor: 'var(--phase-erase-glow)',
    labelKey:  'flash.phaseErase',
  },
  write: {
    gradient:  'linear-gradient(90deg, var(--phase-write-from), var(--phase-write-to))',
    glowColor: 'var(--phase-write-glow)',
    labelKey:  'flash.phaseWrite',
  },
  verify: {
    gradient:  'linear-gradient(90deg, var(--phase-verify-from), var(--phase-verify-to))',
    glowColor: 'var(--phase-verify-glow)',
    labelKey:  'flash.phaseVerify',
  },
  read: {
    gradient:  'linear-gradient(90deg, var(--phase-read-from), var(--phase-read-to))',
    glowColor: 'var(--phase-read-glow)',
    labelKey:  'flash.phaseRead',
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
