import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri'

export type RuntimeType = 'tauri' | 'vscode' | 'web'

export function getRuntime(): RuntimeType {
  if (typeof window === 'undefined') return 'web'
  if (isTauriRuntime()) return 'tauri'
  if ((window as any).__TUYAOPEN_IDE_CONFIG?.runtime === 'vscode') return 'vscode'
  return 'web'
}
