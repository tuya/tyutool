# Batch Flash Tool – Phase 1 (Batch Flashing) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `/toolbox/batch-flash` page that flashes the same firmware to up to 32 serial ports in parallel, with per-port progress, cumulative stats, auto-assign, and port filtering.

**Architecture:** New `src/features/batch-flash/` feature module with a Pinia store, Vue components, and a new `BatchFlashState` added to `src-tauri/src/lib.rs`. Rust spawns one thread per port, emitting `batch-flash-progress` events keyed by port name. Phase 2 (batch auth) builds on this foundation as a separate plan.

**Tech Stack:** Vue 3 Composition API, Pinia, TypeScript, Tailwind CSS v4, `--ty-*` CSS variables, Tauri 2, `tyutool_core::run_job`.

---

## File Map

| File | Action | Purpose |
|---|---|---|
| `src/features/batch-flash/types.ts` | Create | All TS types + `BATCH_AUTH_SUPPORTED_CHIPS` constant |
| `src/features/batch-flash/port-filter.ts` | Create | Port name normalization + filter logic |
| `src/features/batch-flash/port-filter.test.ts` | Create | Unit tests for filter logic |
| `src/features/batch-flash/store.ts` | Create | Pinia store (slots, config, computed, actions) |
| `src/features/batch-flash/store.test.ts` | Create | Unit tests for store logic |
| `src/features/batch-flash/index.ts` | Create | Feature public exports |
| `src/features/batch-flash/BatchFlashPage.vue` | Create | Page entry, lifecycle |
| `src/features/batch-flash/components/BatchDonutChart.vue` | Create | SVG dual-segment donut ring |
| `src/features/batch-flash/components/BatchProgressBar.vue` | Create | Dual-color segmented bar |
| `src/features/batch-flash/components/BatchFlashDashboard.vue` | Create | Cumulative + current stats |
| `src/features/batch-flash/components/BatchFlashConfig.vue` | Create | Chip / baud / firmware config |
| `src/features/batch-flash/components/BatchAuthConfig.vue` | Create | Auth config placeholder (ESP32/GD32 only) |
| `src/features/batch-flash/components/BatchFlashSlotRow.vue` | Create | Single port row |
| `src/features/batch-flash/components/BatchFlashSlotList.vue` | Create | Scrollable list + empty state |
| `src/features/batch-flash/components/BatchFlashToolbar.vue` | Create | Auto-assign, filter badge, action buttons |
| `src/features/batch-flash/components/PortFilterModal.vue` | Create | Port filter dialog |
| `src/router/index.ts` | Modify | Add `/toolbox` + `/toolbox/batch-flash` routes |
| `src/App.vue` | Modify | Add toolbox nav item |
| `src/locales/zh-CN.json` | Modify | Add `batchFlash.*` keys |
| `src/locales/en.json` | Modify | Add `batchFlash.*` keys |
| `src-tauri/src/lib.rs` | Modify | `BatchFlashState`, 3 commands, exit cleanup |

---

## Task 1: Types and port-filter logic

**Files:**
- Create: `src/features/batch-flash/types.ts`
- Create: `src/features/batch-flash/port-filter.ts`

- [ ] **Step 1.1: Create types.ts**

```ts
// src/features/batch-flash/types.ts
import type { FlashProgressPayload } from '@/features/firmware-flash/flash-tauri'

export const BATCH_AUTH_SUPPORTED_CHIPS = ['ESP32', 'GD32'] as const
export type BatchAuthSupportedChip = (typeof BATCH_AUTH_SUPPORTED_CHIPS)[number]

export type BatchOpMode = 'flash-only' | 'auth-only' | 'flash-then-auth'

export type BatchSlotStatus =
  | 'idle'
  | 'flashing'
  | 'reading_mac'
  | 'authorizing'
  | 'done'
  | 'failed'
  | 'skipped'

export interface BatchSlotState {
  port: string
  status: BatchSlotStatus
  progress: number // 0–100
  currentPhase: string
  mac?: string
  error?: string
}

export interface CumulativeStats {
  flash: { total: number; success: number; fail: number }
  auth: { total: number; success: number; fail: number }
}

export interface PortFilterConfig {
  blockedPorts: string[]
}

export interface BatchAuthConfigData {
  excelPath: string
  conflictPolicy: 'skip' | 'overwrite'
}

/** Mirrors Rust `BatchFlashStartConfig` (camelCase via serde). */
export interface BatchFlashStartConfig {
  chipId: string
  baudRate: number
  firmwarePath: string
}

/** `batch-flash-progress` event payload from Rust. */
export interface BatchFlashProgressEvent {
  port: string
  event: FlashProgressPayload
}

export type CompletionBannerKind = 'success' | 'partial' | 'all-failed'

export interface CompletionBanner {
  kind: CompletionBannerKind
  message: string
}
```

- [ ] **Step 1.2: Create port-filter.ts**

```ts
// src/features/batch-flash/port-filter.ts

/** On Windows, port names are case-insensitive (COM3 = com3). Normalize to uppercase. */
export function normalizePortName(port: string): string {
  if (typeof navigator !== 'undefined' && navigator.platform?.toLowerCase().includes('win')) {
    return port.toUpperCase()
  }
  return port
}

/** Remove ports that match any entry in blockedPorts (case-insensitive on Windows). */
export function applyPortFilter(ports: string[], blockedPorts: string[]): string[] {
  if (blockedPorts.length === 0) return ports
  const blocked = new Set(blockedPorts.map(normalizePortName))
  return ports.filter(p => !blocked.has(normalizePortName(p)))
}
```

- [ ] **Step 1.3: Commit**

```bash
git add src/features/batch-flash/types.ts src/features/batch-flash/port-filter.ts
git commit -m "feat(batch-flash): add types and port-filter logic"
```

---

## Task 2: Port-filter tests

**Files:**
- Create: `src/features/batch-flash/port-filter.test.ts`

- [ ] **Step 2.1: Write tests**

```ts
// src/features/batch-flash/port-filter.test.ts
import { describe, it, expect } from 'vitest'
import { normalizePortName, applyPortFilter } from './port-filter'

describe('normalizePortName', () => {
  it('returns the port unchanged on Linux/macOS (no win in platform)', () => {
    // vitest runs in node — navigator.platform is undefined, treated as non-Windows
    expect(normalizePortName('/dev/ttyUSB0')).toBe('/dev/ttyUSB0')
    expect(normalizePortName('COM3')).toBe('COM3')
  })
})

describe('applyPortFilter', () => {
  it('returns all ports when blockedPorts is empty', () => {
    const ports = ['/dev/ttyUSB0', '/dev/ttyUSB1']
    expect(applyPortFilter(ports, [])).toEqual(ports)
  })

  it('removes a single blocked port', () => {
    expect(
      applyPortFilter(['/dev/ttyUSB0', '/dev/ttyS0', '/dev/ttyUSB1'], ['/dev/ttyS0'])
    ).toEqual(['/dev/ttyUSB0', '/dev/ttyUSB1'])
  })

  it('removes multiple blocked ports', () => {
    expect(
      applyPortFilter(['COM1', 'COM3', 'COM5'], ['COM1', 'COM5'])
    ).toEqual(['COM3'])
  })

  it('returns empty array when all ports are blocked', () => {
    expect(applyPortFilter(['COM1', 'COM2'], ['COM1', 'COM2'])).toEqual([])
  })

  it('does not remove ports that are not blocked', () => {
    expect(
      applyPortFilter(['/dev/ttyUSB0', '/dev/ttyUSB1'], ['/dev/ttyACM0'])
    ).toEqual(['/dev/ttyUSB0', '/dev/ttyUSB1'])
  })
})
```

- [ ] **Step 2.2: Run and confirm pass**

```bash
pnpm exec vitest run src/features/batch-flash/port-filter.test.ts
```

Expected: all tests PASS.

- [ ] **Step 2.3: Commit**

```bash
git add src/features/batch-flash/port-filter.test.ts
git commit -m "test(batch-flash): add port-filter unit tests"
```

---

## Task 3: Pinia store

**Files:**
- Create: `src/features/batch-flash/store.ts`

- [ ] **Step 3.1: Create store.ts**

```ts
// src/features/batch-flash/store.ts
import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri'
import {
  BATCH_AUTH_SUPPORTED_CHIPS,
  type BatchSlotState,
  type BatchSlotStatus,
  type CumulativeStats,
  type PortFilterConfig,
  type BatchAuthConfigData,
  type BatchFlashProgressEvent,
  type BatchFlashStartConfig,
  type BatchOpMode,
  type CompletionBanner,
} from './types'
import { applyPortFilter, normalizePortName } from './port-filter'

const CUMULATIVE_KEY = 'batch-flash-cumulative'
const FILTER_KEY = 'batch-flash-port-filter'
const STORE_FILE = 'settings.json'

const ACTIVE_STATUSES: BatchSlotStatus[] = ['flashing', 'reading_mac', 'authorizing']
const TERMINAL_STATUSES: BatchSlotStatus[] = ['done', 'failed', 'skipped', 'idle']

export const useBatchFlashStore = defineStore('batch-flash', () => {
  // ── Persisted config ──────────────────────────────────────────────────────
  const filterConfig = ref<PortFilterConfig>({ blockedPorts: [] })
  const cumulativeStats = ref<CumulativeStats>({
    flash: { total: 0, success: 0, fail: 0 },
    auth: { total: 0, success: 0, fail: 0 },
  })

  // ── Session state ─────────────────────────────────────────────────────────
  const slots = ref<BatchSlotState[]>([])
  const chipId = ref<string>('ESP32')
  const baudRate = ref<number>(115200)
  const firmwarePath = ref<string>('')
  const authConfig = ref<BatchAuthConfigData>({ excelPath: '', conflictPolicy: 'skip' })
  const batchStartTime = ref<number | null>(null)
  const completionBanner = ref<CompletionBanner | null>(null)

  let unlisten: (() => void) | undefined

  // ── Computed ──────────────────────────────────────────────────────────────
  const authSupported = computed(() =>
    (BATCH_AUTH_SUPPORTED_CHIPS as readonly string[]).includes(chipId.value)
  )

  const opMode = computed<BatchOpMode>(() => {
    const hasFirmware = !!firmwarePath.value
    const hasExcel = authSupported.value && !!authConfig.value.excelPath
    if (hasFirmware && hasExcel) return 'flash-then-auth'
    if (hasExcel) return 'auth-only'
    return 'flash-only'
  })

  const showAuthStats = computed(() => opMode.value !== 'flash-only')

  const currentStats = computed(() => ({
    active: slots.value.filter(s => ACTIVE_STATUSES.includes(s.status)).length,
    done: slots.value.filter(s => s.status === 'done').length,
    failed: slots.value.filter(s => s.status === 'failed').length,
    skipped: slots.value.filter(s => s.status === 'skipped').length,
  }))

  const inputsValid = computed(() => {
    if (opMode.value !== 'auth-only' && !firmwarePath.value) return false
    if (opMode.value !== 'flash-only' && !authConfig.value.excelPath) return false
    return true
  })

  const isBusy = computed(() => currentStats.value.active > 0)
  const canStart = computed(() =>
    slots.value.some(s => s.status === 'idle') && inputsValid.value
  )
  const canCancel = computed(() => isBusy.value)
  const canRetry = computed(() => slots.value.some(s => s.status === 'failed'))
  const filterActive = computed(() => filterConfig.value.blockedPorts.length > 0)

  // ── Slot helpers ──────────────────────────────────────────────────────────
  function findSlot(port: string): BatchSlotState | undefined {
    return slots.value.find(s => s.port === port)
  }

  function updateSlot(port: string, patch: Partial<BatchSlotState>) {
    const slot = findSlot(port)
    if (slot) Object.assign(slot, patch)
  }

  // ── Port management ───────────────────────────────────────────────────────
  function addPorts(ports: string[]) {
    const existing = new Set(slots.value.map(s => s.port))
    for (const port of ports) {
      if (!existing.has(port)) {
        slots.value.push({ port, status: 'idle', progress: 0, currentPhase: '' })
        existing.add(port)
      }
    }
  }

  function removeSlot(port: string) {
    const slot = findSlot(port)
    if (!slot) return
    if (slot.status === 'idle' || slot.status === 'done') {
      slots.value = slots.value.filter(s => s.port !== port)
    }
  }

  async function autoAssign() {
    if (!isTauriRuntime()) return
    const { invoke } = await import('@tauri-apps/api/core')
    const all: Array<{ path: string }> = await invoke('list_serial_ports_cmd')
    const filtered = applyPortFilter(
      all.map(p => p.path),
      filterConfig.value.blockedPorts
    )
    addPorts(filtered)
  }

  // ── Flash actions ─────────────────────────────────────────────────────────
  async function startFlash() {
    if (!canStart.value) return
    batchStartTime.value = Date.now()
    completionBanner.value = null

    const idlePorts = slots.value.filter(s => s.status === 'idle').map(s => s.port)
    for (const port of idlePorts) {
      updateSlot(port, { status: 'flashing', progress: 0, currentPhase: '', error: undefined })
    }

    if (!isTauriRuntime()) return
    const { invoke } = await import('@tauri-apps/api/core')
    const config: BatchFlashStartConfig = {
      chipId: chipId.value,
      baudRate: baudRate.value,
      firmwarePath: firmwarePath.value,
    }
    await invoke('batch_flash_start', { config, ports: idlePorts })
  }

  async function retryFailed() {
    if (!canRetry.value) return
    completionBanner.value = null
    for (const slot of slots.value.filter(s => s.status === 'failed')) {
      updateSlot(slot.port, { status: 'idle', progress: 0, currentPhase: '', error: undefined })
    }
    await startFlash()
  }

  async function cancelPort(port: string) {
    if (!isTauriRuntime()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('batch_flash_cancel_port', { port })
  }

  async function cancelAll() {
    if (!isTauriRuntime()) return
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('batch_flash_cancel_all')
  }

  // ── Progress event handler ────────────────────────────────────────────────
  function handleFlashProgress(ev: BatchFlashProgressEvent) {
    const { port, event: e } = ev
    if (!findSlot(port)) return

    if (e.kind === 'percent') {
      updateSlot(port, { progress: e.value })
    } else if (e.kind === 'phase') {
      updateSlot(port, { currentPhase: String(e.phase) })
    } else if (e.kind === 'done') {
      const r = e.result
      if ('ok' in r) {
        updateSlot(port, { status: 'done', progress: 100, currentPhase: '' })
        cumulativeStats.value.flash.total++
        cumulativeStats.value.flash.success++
      } else if ('err' in r) {
        updateSlot(port, { status: 'failed', error: r.err.message })
        cumulativeStats.value.flash.total++
        cumulativeStats.value.flash.fail++
      } else {
        // cancelled — not counted in cumulative
        updateSlot(port, { status: 'idle', progress: 0, currentPhase: '' })
      }
      void saveCumulativeStats()
      checkBatchCompletion()
    }
  }

  function checkBatchCompletion() {
    const anyActive = slots.value.some(s => ACTIVE_STATUSES.includes(s.status))
    if (anyActive || batchStartTime.value === null) return

    const { done, failed } = currentStats.value
    if (failed === 0) {
      completionBanner.value = { kind: 'success', message: `本次批次完成：${done} 台全部成功` }
    } else if (done === 0) {
      completionBanner.value = { kind: 'all-failed', message: `本次批次全部失败，请检查连接后重试` }
    } else {
      completionBanner.value = {
        kind: 'partial',
        message: `本次批次完成：${done} 成功，${failed} 失败，可点击「重试失败」`,
      }
    }
  }

  function dismissBanner() { completionBanner.value = null }

  // ── Port filter ───────────────────────────────────────────────────────────
  function addBlockedPort(port: string) {
    const normalized = normalizePortName(port)
    if (!filterConfig.value.blockedPorts.includes(normalized)) {
      filterConfig.value.blockedPorts.push(normalized)
    }
    // Remove idle slots that are now filtered
    slots.value = slots.value.filter(
      s => s.status !== 'idle' ||
        !filterConfig.value.blockedPorts.includes(normalizePortName(s.port))
    )
    void saveFilterConfig()
  }

  function removeBlockedPort(port: string) {
    const normalized = normalizePortName(port)
    filterConfig.value.blockedPorts = filterConfig.value.blockedPorts.filter(p => p !== normalized)
    void saveFilterConfig()
  }

  // ── Cumulative stats reset ────────────────────────────────────────────────
  function resetFlashStats() {
    cumulativeStats.value.flash = { total: 0, success: 0, fail: 0 }
    void saveCumulativeStats()
  }

  function resetAuthStats() {
    cumulativeStats.value.auth = { total: 0, success: 0, fail: 0 }
    void saveCumulativeStats()
  }

  // ── Persistence ───────────────────────────────────────────────────────────
  async function loadPersistedData() {
    if (!isTauriRuntime()) return
    const { Store } = await import('@tauri-apps/plugin-store')
    const store = new Store(STORE_FILE)
    const cumulative = await store.get<CumulativeStats>(CUMULATIVE_KEY)
    if (cumulative) cumulativeStats.value = cumulative
    const filter = await store.get<PortFilterConfig>(FILTER_KEY)
    if (filter) filterConfig.value = filter
  }

  async function saveCumulativeStats() {
    if (!isTauriRuntime()) return
    const { Store } = await import('@tauri-apps/plugin-store')
    const store = new Store(STORE_FILE)
    await store.set(CUMULATIVE_KEY, cumulativeStats.value)
    await store.save()
  }

  async function saveFilterConfig() {
    if (!isTauriRuntime()) return
    const { Store } = await import('@tauri-apps/plugin-store')
    const store = new Store(STORE_FILE)
    await store.set(FILTER_KEY, filterConfig.value)
    await store.save()
  }

  // ── Event listener lifecycle ──────────────────────────────────────────────
  async function ensureListener() {
    if (unlisten || !isTauriRuntime()) return
    const { listen } = await import('@tauri-apps/api/event')
    unlisten = await listen<BatchFlashProgressEvent>('batch-flash-progress', ({ payload }) => {
      handleFlashProgress(payload)
    })
  }

  function cleanup() {
    unlisten?.()
    unlisten = undefined
  }

  return {
    // State
    slots, chipId, baudRate, firmwarePath, authConfig,
    filterConfig, cumulativeStats, completionBanner,
    // Computed
    authSupported, opMode, showAuthStats, currentStats,
    inputsValid, isBusy, canStart, canCancel, canRetry, filterActive,
    batchStartTime,
    // Actions
    addPorts, removeSlot, autoAssign,
    startFlash, retryFailed, cancelPort, cancelAll,
    addBlockedPort, removeBlockedPort,
    resetFlashStats, resetAuthStats, dismissBanner,
    loadPersistedData, ensureListener, cleanup,
    // Internal (exposed for testing)
    handleFlashProgress,
  }
})
```

- [ ] **Step 3.2: Create index.ts**

```ts
// src/features/batch-flash/index.ts
export { useBatchFlashStore } from './store'
export type { BatchSlotState, BatchOpMode, BatchSlotStatus } from './types'
export { BATCH_AUTH_SUPPORTED_CHIPS } from './types'
```

- [ ] **Step 3.3: Commit**

```bash
git add src/features/batch-flash/store.ts src/features/batch-flash/index.ts
git commit -m "feat(batch-flash): add Pinia store"
```

---

## Task 4: Store unit tests

**Files:**
- Create: `src/features/batch-flash/store.test.ts`

- [ ] **Step 4.1: Write store tests**

```ts
// src/features/batch-flash/store.test.ts
import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useBatchFlashStore } from './store'

vi.mock('@/features/firmware-flash/flash-tauri', () => ({
  isTauriRuntime: () => false,
}))

describe('opMode', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('is flash-only when no firmware and no excel', () => {
    const store = useBatchFlashStore()
    expect(store.opMode).toBe('flash-only')
  })

  it('is flash-only when firmware selected but no excel', () => {
    const store = useBatchFlashStore()
    store.firmwarePath = '/path/to/fw.bin'
    expect(store.opMode).toBe('flash-only')
  })

  it('is flash-only when excel selected but chip does not support auth', () => {
    const store = useBatchFlashStore()
    store.chipId = 'BK7231N'
    store.authConfig.excelPath = '/path/to/auth.xlsx'
    expect(store.opMode).toBe('flash-only')
  })

  it('is auth-only when excel selected and chip supports auth, no firmware', () => {
    const store = useBatchFlashStore()
    store.chipId = 'ESP32'
    store.authConfig.excelPath = '/path/to/auth.xlsx'
    expect(store.opMode).toBe('auth-only')
  })

  it('is flash-then-auth when both firmware and excel are set with supported chip', () => {
    const store = useBatchFlashStore()
    store.chipId = 'GD32'
    store.firmwarePath = '/path/to/fw.bin'
    store.authConfig.excelPath = '/path/to/auth.xlsx'
    expect(store.opMode).toBe('flash-then-auth')
  })
})

describe('authSupported', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('is true for ESP32', () => {
    const store = useBatchFlashStore()
    store.chipId = 'ESP32'
    expect(store.authSupported).toBe(true)
  })

  it('is true for GD32', () => {
    const store = useBatchFlashStore()
    store.chipId = 'GD32'
    expect(store.authSupported).toBe(true)
  })

  it('is false for BK7231N', () => {
    const store = useBatchFlashStore()
    store.chipId = 'BK7231N'
    expect(store.authSupported).toBe(false)
  })
})

describe('slot state machine', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('addPorts creates idle slots', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3', 'COM5'])
    expect(store.slots).toHaveLength(2)
    expect(store.slots[0]).toMatchObject({ port: 'COM3', status: 'idle', progress: 0 })
  })

  it('addPorts deduplicates existing ports', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.addPorts(['COM3', 'COM5'])
    expect(store.slots).toHaveLength(2)
  })

  it('removeSlot removes idle slots', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.removeSlot('COM3')
    expect(store.slots).toHaveLength(0)
  })

  it('removeSlot does not remove flashing slots', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.slots[0].status = 'flashing'
    store.removeSlot('COM3')
    expect(store.slots).toHaveLength(1)
  })

  it('handleFlashProgress percent updates slot progress', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.slots[0].status = 'flashing'
    store.handleFlashProgress({ port: 'COM3', event: { kind: 'percent', value: 68 } })
    expect(store.slots[0].progress).toBe(68)
  })

  it('handleFlashProgress done/ok transitions slot to done and increments cumulative', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.slots[0].status = 'flashing'
    store.batchStartTime = Date.now()
    store.handleFlashProgress({
      port: 'COM3',
      event: { kind: 'done', result: { ok: { elapsed_secs: 10 } } },
    })
    expect(store.slots[0].status).toBe('done')
    expect(store.cumulativeStats.flash.total).toBe(1)
    expect(store.cumulativeStats.flash.success).toBe(1)
    expect(store.cumulativeStats.flash.fail).toBe(0)
  })

  it('handleFlashProgress done/err transitions slot to failed and increments cumulative', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.slots[0].status = 'flashing'
    store.batchStartTime = Date.now()
    store.handleFlashProgress({
      port: 'COM3',
      event: { kind: 'done', result: { err: { message: 'timeout', elapsed_secs: 5 } } },
    })
    expect(store.slots[0].status).toBe('failed')
    expect(store.slots[0].error).toBe('timeout')
    expect(store.cumulativeStats.flash.total).toBe(1)
    expect(store.cumulativeStats.flash.fail).toBe(1)
  })

  it('handleFlashProgress done/cancelled resets slot to idle without incrementing cumulative', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.slots[0].status = 'flashing'
    store.handleFlashProgress({
      port: 'COM3',
      event: { kind: 'done', result: { cancelled: { elapsed_secs: 2 } } },
    })
    expect(store.slots[0].status).toBe('idle')
    expect(store.cumulativeStats.flash.total).toBe(0)
  })
})

describe('canStart / canRetry / canCancel', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('canStart is false when no slots', () => {
    const store = useBatchFlashStore()
    store.firmwarePath = '/fw.bin'
    expect(store.canStart).toBe(false)
  })

  it('canStart is false when firmware missing (flash-only mode)', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    expect(store.canStart).toBe(false)
  })

  it('canStart is true when idle slot exists and inputs valid', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.firmwarePath = '/fw.bin'
    expect(store.canStart).toBe(true)
  })

  it('canRetry is false when no failed slots', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    expect(store.canRetry).toBe(false)
  })

  it('canRetry is true when any slot is failed', () => {
    const store = useBatchFlashStore()
    store.addPorts(['COM3'])
    store.slots[0].status = 'failed'
    expect(store.canRetry).toBe(true)
  })
})

describe('completionBanner', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('shows success banner when all done', () => {
    const store = useBatchFlashStore()
    store.batchStartTime = Date.now()
    store.addPorts(['COM3', 'COM5'])
    store.slots.forEach(s => { s.status = 'flashing' })
    // Simulate both completing
    store.handleFlashProgress({ port: 'COM3', event: { kind: 'done', result: { ok: { elapsed_secs: 5 } } } })
    store.handleFlashProgress({ port: 'COM5', event: { kind: 'done', result: { ok: { elapsed_secs: 5 } } } })
    expect(store.completionBanner?.kind).toBe('success')
  })

  it('shows partial banner on mixed outcome', () => {
    const store = useBatchFlashStore()
    store.batchStartTime = Date.now()
    store.addPorts(['COM3', 'COM5'])
    store.slots.forEach(s => { s.status = 'flashing' })
    store.handleFlashProgress({ port: 'COM3', event: { kind: 'done', result: { ok: { elapsed_secs: 5 } } } })
    store.handleFlashProgress({ port: 'COM5', event: { kind: 'done', result: { err: { message: 'fail', elapsed_secs: 5 } } } })
    expect(store.completionBanner?.kind).toBe('partial')
  })
})

describe('resetFlashStats', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('resets flash cumulative to zero', () => {
    const store = useBatchFlashStore()
    store.cumulativeStats.flash = { total: 10, success: 8, fail: 2 }
    store.resetFlashStats()
    expect(store.cumulativeStats.flash).toEqual({ total: 0, success: 0, fail: 0 })
  })
})
```

- [ ] **Step 4.2: Run tests**

```bash
pnpm exec vitest run src/features/batch-flash/store.test.ts
```

Expected: all tests PASS.

- [ ] **Step 4.3: Commit**

```bash
git add src/features/batch-flash/store.test.ts
git commit -m "test(batch-flash): add store unit tests"
```

---

## Task 5: Routing + navigation

**Files:**
- Modify: `src/router/index.ts`
- Modify: `src/App.vue`

- [ ] **Step 5.1: Add routes to router/index.ts**

In `src/router/index.ts`, add after the `serial-debug` route:

```ts
import BatchFlashPage from '@/features/batch-flash/BatchFlashPage.vue';
```

Add to the `routes` array:

```ts
{ path: '/toolbox', redirect: '/toolbox/batch-flash' },
{
  path: '/toolbox/batch-flash',
  name: 'batch-flash',
  component: BatchFlashPage,
  meta: { title: '批量烧录', layout: 'fullBleed' }
},
```

- [ ] **Step 5.2: Add nav item to App.vue**

In `src/App.vue`, add to the `nav` computed array (before settings):

```ts
{
  name: 'batch-flash' as const,
  to: '/toolbox/batch-flash',
  label: t('app.nav.toolbox'),
  faIcon: ['fas', 'layer-group'] as [string, string],
},
```

- [ ] **Step 5.3: Create BatchFlashPage.vue stub** (so the import doesn't break)

```vue
<!-- src/features/batch-flash/BatchFlashPage.vue -->
<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { useBatchFlashStore } from './store'

const store = useBatchFlashStore()

onMounted(async () => {
  await store.loadPersistedData()
  await store.ensureListener()
})

onUnmounted(() => {
  if (!store.isBusy) store.cleanup()
})
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-3">
    <p class="text-[var(--ty-text-muted)]">批量烧录（开发中）</p>
  </div>
</template>
```

- [ ] **Step 5.4: Add i18n key for nav**

In `src/locales/zh-CN.json`, inside `"app": { "nav": { ... } }`:
```json
"toolbox": "工具箱"
```

In `src/locales/en.json`, inside `"app": { "nav": { ... } }`:
```json
"toolbox": "Toolbox"
```

- [ ] **Step 5.5: Verify app builds**

```bash
pnpm run build
```

Expected: no TypeScript errors.

- [ ] **Step 5.6: Commit**

```bash
git add src/router/index.ts src/App.vue src/features/batch-flash/BatchFlashPage.vue src/locales/
git commit -m "feat(batch-flash): add route and nav item"
```

---

## Task 6: Rust backend – BatchFlashState + commands

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 6.1: Add BatchFlashState struct** (after the existing `DebugState` struct)

```rust
struct BatchSlot {
    cancel: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

struct BatchFlashState {
    /// key = port name (OS-native format, as received from frontend)
    slots: StdMutex<HashMap<String, BatchSlot>>,
}
```

Add import at top of file (alongside existing imports):
```rust
use std::collections::HashMap;
```

- [ ] **Step 6.2: Add batch_flash_start command**

```rust
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BatchFlashStartConfig {
    chip_id: String,
    baud_rate: u32,
    firmware_path: String,
}

#[tauri::command]
fn batch_flash_start(
    app: AppHandle,
    state: State<'_, BatchFlashState>,
    config: BatchFlashStartConfig,
    ports: Vec<String>,
) -> Result<(), String> {
    let mut slots = state.slots.lock().map_err(|e| e.to_string())?;

    for port in ports {
        // Wait for any previous thread on this port to exit (up to 3s)
        if let Some(old) = slots.remove(&port) {
            old.cancel.store(true, Ordering::SeqCst);
            let (tx, rx) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                let _ = old.thread.join();
                let _ = tx.send(());
            });
            if rx.recv_timeout(Duration::from_secs(3)).is_err() {
                return Err(format!(
                    "port {} previous operation not stopped; retry in a few seconds",
                    port
                ));
            }
        }

        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_clone = cancel.clone();
        let app_clone = app.clone();
        let port_clone = port.clone();
        let config_clone = config.clone();

        let handle = std::thread::spawn(move || {
            if !std::path::Path::new(&config_clone.firmware_path).exists() {
                let _ = app_clone.emit(
                    "batch-flash-progress",
                    serde_json::json!({
                        "port": port_clone,
                        "event": { "kind": "done", "result": { "err": { "message": "firmware file not found", "elapsed_secs": 0 } } }
                    }),
                );
                return;
            }

            let job = tyutool_core::FlashJob {
                mode: tyutool_core::FlashMode::Flash,
                chip_id: config_clone.chip_id.clone(),
                port: port_clone.clone(),
                baud_rate: config_clone.baud_rate,
                firmware_path: Some(config_clone.firmware_path.clone()),
                segments: None,
                flash_start_hex: None,
                flash_end_hex: None,
                erase_start_hex: None,
                erase_end_hex: None,
                read_start_hex: None,
                read_end_hex: None,
                read_file_path: None,
                authorize_uuid: None,
                authorize_key: None,
            };

            let _ = tyutool_core::run_job(&job, &cancel_clone, |p| {
                let _ = app_clone.emit(
                    "batch-flash-progress",
                    serde_json::json!({ "port": port_clone, "event": p }),
                );
            });
        });

        slots.insert(port, BatchSlot { cancel, thread: handle });
    }

    Ok(())
}
```

- [ ] **Step 6.3: Add batch_flash_cancel_port and batch_flash_cancel_all**

```rust
#[tauri::command]
fn batch_flash_cancel_port(
    state: State<'_, BatchFlashState>,
    port: String,
) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    if let Some(slot) = slots.get(&port) {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}

#[tauri::command]
fn batch_flash_cancel_all(state: State<'_, BatchFlashState>) -> Result<(), String> {
    let slots = state.slots.lock().map_err(|e| e.to_string())?;
    for slot in slots.values() {
        slot.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}
```

- [ ] **Step 6.4: Register BatchFlashState and commands**

In `.manage(...)` chain (after the existing `DebugState`):
```rust
.manage(BatchFlashState {
    slots: StdMutex::new(HashMap::new()),
})
```

In `.invoke_handler(tauri::generate_handler![...])`, add:
```rust
batch_flash_start,
batch_flash_cancel_port,
batch_flash_cancel_all,
```

- [ ] **Step 6.5: Add exit cleanup to RunEvent::ExitRequested**

In the `run(|app_handle, event|` closure, add a new match arm:

```rust
RunEvent::ExitRequested { .. } => {
    // Cancel all batch flash threads and give them up to 5s to release serial ports
    if let Some(batch_state) = app_handle.try_state::<BatchFlashState>() {
        if let Ok(slots) = batch_state.slots.lock() {
            for slot in slots.values() {
                slot.cancel.store(true, Ordering::SeqCst);
            }
        }
    }
}
```

- [ ] **Step 6.6: Build Rust to verify no errors**

```bash
cargo build -p tyutool-core 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 6.7: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(batch-flash): add Rust BatchFlashState and batch_flash_* commands"
```

---

## Task 7: BatchDonutChart + BatchProgressBar components

**Files:**
- Create: `src/features/batch-flash/components/BatchDonutChart.vue`
- Create: `src/features/batch-flash/components/BatchProgressBar.vue`

- [ ] **Step 7.1: Create BatchDonutChart.vue**

```vue
<!-- src/features/batch-flash/components/BatchDonutChart.vue -->
<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  total: number
  success: number
  fail: number
}>()

// r=48, circumference = 2π×48 ≈ 301.59
const C = 2 * Math.PI * 48

const successArc = computed(() => {
  if (props.total === 0) return 0
  return Math.min(C * (props.success / props.total), C)
})

const failArc = computed(() => {
  if (props.total === 0) return 0
  return Math.min(C * (props.fail / props.total), C - successArc.value)
})

const pct = computed(() =>
  props.total === 0 ? '-' : Math.round((props.success / props.total) * 100) + '%'
)
</script>

<template>
  <svg viewBox="0 0 120 120" class="w-20 h-20 shrink-0" aria-hidden="true">
    <!-- Background ring -->
    <circle
      cx="60" cy="60" r="48"
      fill="none"
      stroke="var(--ty-border)"
      stroke-width="14"
    />
    <!-- Success arc -->
    <circle
      v-if="total > 0"
      cx="60" cy="60" r="48"
      fill="none"
      stroke="var(--ty-success)"
      stroke-width="14"
      :stroke-dasharray="`${successArc} ${C - successArc}`"
      stroke-linecap="butt"
      transform="rotate(-90 60 60)"
    />
    <!-- Fail arc -->
    <circle
      v-if="total > 0 && failArc > 0"
      cx="60" cy="60" r="48"
      fill="none"
      stroke="var(--ty-danger)"
      stroke-width="14"
      :stroke-dasharray="`${failArc} ${C - failArc}`"
      :stroke-dashoffset="`${-successArc}`"
      stroke-linecap="butt"
      transform="rotate(-90 60 60)"
    />
    <!-- Center text -->
    <text
      x="60" y="56"
      text-anchor="middle"
      font-size="18"
      font-weight="600"
      fill="var(--ty-text)"
    >{{ pct }}</text>
    <text
      x="60" y="72"
      text-anchor="middle"
      font-size="11"
      fill="var(--ty-text-muted)"
    >成功率</text>
  </svg>
</template>
```

- [ ] **Step 7.2: Create BatchProgressBar.vue**

```vue
<!-- src/features/batch-flash/components/BatchProgressBar.vue -->
<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{
  total: number
  success: number
  fail: number
}>()

const successPct = computed(() =>
  props.total === 0 ? 0 : (props.success / props.total) * 100
)
const failPct = computed(() =>
  props.total === 0 ? 0 : (props.fail / props.total) * 100
)
</script>

<template>
  <div
    class="relative h-2 w-full overflow-hidden rounded-full"
    :style="{ backgroundColor: 'var(--ty-border)' }"
    role="progressbar"
    :aria-valuenow="success"
    :aria-valuemax="total"
  >
    <div
      class="absolute inset-y-0 left-0 transition-all duration-300"
      :style="{
        width: successPct + '%',
        backgroundColor: 'var(--ty-success)',
      }"
    />
    <div
      class="absolute inset-y-0 transition-all duration-300"
      :style="{
        left: successPct + '%',
        width: failPct + '%',
        backgroundColor: 'var(--ty-danger)',
      }"
    />
  </div>
</template>
```

- [ ] **Step 7.3: Commit**

```bash
git add src/features/batch-flash/components/BatchDonutChart.vue \
        src/features/batch-flash/components/BatchProgressBar.vue
git commit -m "feat(batch-flash): add BatchDonutChart and BatchProgressBar components"
```

---

## Task 8: BatchFlashSlotRow + BatchFlashSlotList

**Files:**
- Create: `src/features/batch-flash/components/BatchFlashSlotRow.vue`
- Create: `src/features/batch-flash/components/BatchFlashSlotList.vue`

- [ ] **Step 8.1: Create BatchFlashSlotRow.vue**

```vue
<!-- src/features/batch-flash/components/BatchFlashSlotRow.vue -->
<script setup lang="ts">
import { computed } from 'vue'
import type { BatchSlotState } from '../types'

const props = defineProps<{ slot: BatchSlotState }>()
const emit = defineEmits<{
  cancel: [port: string]
  retry: [port: string]
  remove: [port: string]
}>()

const statusLabel = computed(() => ({
  idle: '空闲', flashing: '烧录中',
  reading_mac: '读取MAC', authorizing: '写入授权',
  done: '成功', failed: '失败', skipped: '已跳过',
}[props.slot.status] ?? props.slot.status))

const statusColor = computed(() => ({
  idle: 'var(--ty-text-muted)',
  flashing: 'var(--ty-primary)',
  reading_mac: 'var(--ty-primary)',
  authorizing: 'var(--ty-primary)',
  done: 'var(--ty-success)',
  failed: 'var(--ty-danger)',
  skipped: 'var(--ty-text-muted)',
}[props.slot.status]))

const borderColor = computed(() => ({
  idle: 'transparent',
  flashing: 'var(--ty-primary)',
  reading_mac: 'var(--ty-primary)',
  authorizing: 'var(--ty-primary)',
  done: 'var(--ty-success)',
  failed: 'var(--ty-danger)',
  skipped: 'var(--ty-text-muted)',
}[props.slot.status]))

const rowBg = computed(() =>
  props.slot.status === 'failed'
    ? 'color-mix(in srgb, var(--ty-danger) 6%, transparent)'
    : 'transparent'
)

const isActive = computed(() =>
  ['flashing', 'reading_mac', 'authorizing'].includes(props.slot.status)
)

const showProgress = computed(() => isActive.value && props.slot.progress > 0)
</script>

<template>
  <div
    class="flex h-10 min-w-0 items-center gap-3 border-l-[3px] px-3 text-sm transition-colors"
    :style="{
      borderLeftColor: borderColor,
      backgroundColor: rowBg,
    }"
  >
    <!-- Port name -->
    <span class="w-20 shrink-0 font-mono text-xs text-[var(--ty-text)]">{{ slot.port }}</span>

    <!-- Status label -->
    <span
      class="w-24 shrink-0 text-xs font-medium"
      :style="{ color: statusColor }"
    >
      <span
        v-if="isActive"
        class="mr-1 inline-block h-1.5 w-1.5 animate-pulse rounded-full"
        :style="{ backgroundColor: statusColor }"
      />
      {{ statusLabel }}
    </span>

    <!-- Progress bar + percent (active states) -->
    <div v-if="showProgress" class="flex min-w-0 flex-1 items-center gap-2">
      <div class="h-1 flex-1 overflow-hidden rounded-full" :style="{ backgroundColor: 'var(--ty-border)' }">
        <div
          class="h-full rounded-full transition-all duration-200"
          :style="{ width: slot.progress + '%', backgroundColor: 'var(--ty-primary)' }"
        />
      </div>
      <span class="w-9 shrink-0 text-right text-xs text-[var(--ty-text-muted)]">
        {{ slot.progress }}%
      </span>
    </div>

    <!-- Error summary (failed state) -->
    <div v-else-if="slot.status === 'failed' && slot.error" class="flex min-w-0 flex-1 items-center gap-1">
      <span class="min-w-0 truncate text-xs text-[var(--ty-danger)]">{{ slot.error }}</span>
      <button
        type="button"
        class="ml-1 shrink-0 cursor-pointer text-[var(--ty-text-muted)] hover:text-[var(--ty-text)]"
        :title="slot.error"
        aria-label="查看完整错误"
      >ⓘ</button>
    </div>

    <div v-else class="flex-1" />

    <!-- Action buttons -->
    <div class="flex shrink-0 items-center gap-1">
      <button
        v-if="isActive"
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        @click="$emit('cancel', slot.port)"
      >取消</button>
      <button
        v-if="slot.status === 'failed'"
        type="button"
        class="ty-btn-secondary min-h-7 px-2 py-0.5 text-xs"
        @click="$emit('retry', slot.port)"
      >重试</button>
      <button
        v-if="slot.status === 'idle' || slot.status === 'done'"
        type="button"
        class="ty-btn-secondary min-h-7 cursor-pointer px-2 py-0.5 text-xs text-[var(--ty-text-muted)]"
        @click="$emit('remove', slot.port)"
      >删除</button>
    </div>
  </div>
</template>
```

- [ ] **Step 8.2: Create BatchFlashSlotList.vue**

```vue
<!-- src/features/batch-flash/components/BatchFlashSlotList.vue -->
<script setup lang="ts">
import { useBatchFlashStore } from '../store'
import BatchFlashSlotRow from './BatchFlashSlotRow.vue'

const store = useBatchFlashStore()

async function onCancel(port: string) { await store.cancelPort(port) }
async function onRetry(port: string) {
  const slot = store.slots.find(s => s.port === port)
  if (slot) {
    slot.status = 'idle'
    slot.progress = 0
    slot.error = undefined
    await store.startFlash()
  }
}
function onRemove(port: string) { store.removeSlot(port) }
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl border border-[var(--ty-border)]">
    <!-- Header row -->
    <div class="flex h-8 items-center gap-3 border-b border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-3 text-xs font-medium text-[var(--ty-text-muted)]">
      <span class="w-20 shrink-0">串口</span>
      <span class="w-24 shrink-0">状态</span>
      <span class="flex-1">进度</span>
      <span class="shrink-0">操作</span>
    </div>

    <!-- Slot rows -->
    <div v-if="store.slots.length > 0" class="min-h-0 flex-1 overflow-y-auto">
      <BatchFlashSlotRow
        v-for="slot in store.slots"
        :key="slot.port"
        :slot="slot"
        class="border-b border-[var(--ty-border)] last:border-b-0"
        @cancel="onCancel"
        @retry="onRetry"
        @remove="onRemove"
      />
    </div>

    <!-- Empty state -->
    <div v-else class="flex flex-1 flex-col items-center justify-center gap-2 py-10 text-[var(--ty-text-muted)]">
      <FontAwesomeIcon :icon="['fas', 'plug']" class="text-3xl opacity-40" />
      <p class="text-sm">暂无串口</p>
      <p class="text-xs">点击「自动分配」扫描可用串口，或手动添加</p>
    </div>
  </div>
</template>
```

- [ ] **Step 8.3: Commit**

```bash
git add src/features/batch-flash/components/BatchFlashSlotRow.vue \
        src/features/batch-flash/components/BatchFlashSlotList.vue
git commit -m "feat(batch-flash): add slot row and list components"
```

---

## Task 9: BatchFlashDashboard

**Files:**
- Create: `src/features/batch-flash/components/BatchFlashDashboard.vue`

- [ ] **Step 9.1: Create BatchFlashDashboard.vue**

```vue
<!-- src/features/batch-flash/components/BatchFlashDashboard.vue -->
<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue'
import { useBatchFlashStore } from '../store'
import BatchDonutChart from './BatchDonutChart.vue'

const store = useBatchFlashStore()

// Elapsed time ticker
const elapsedDisplay = ref('--:--:--')
let ticker: ReturnType<typeof setInterval> | undefined

function formatElapsed(ms: number): string {
  const s = Math.floor(ms / 1000)
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = s % 60
  return [h, m, sec].map(n => String(n).padStart(2, '0')).join(':')
}

onMounted(() => {
  ticker = setInterval(() => {
    if (store.batchStartTime !== null) {
      elapsedDisplay.value = formatElapsed(Date.now() - store.batchStartTime)
    }
  }, 1000)
})
onUnmounted(() => clearInterval(ticker))

const bannerBg = computed(() => {
  const k = store.completionBanner?.kind
  if (k === 'success') return 'color-mix(in srgb, var(--ty-success) 10%, transparent)'
  if (k === 'all-failed') return 'color-mix(in srgb, var(--ty-danger) 10%, transparent)'
  return 'color-mix(in srgb, var(--ty-accent) 10%, transparent)'
})

const bannerTextColor = computed(() => {
  const k = store.completionBanner?.kind
  if (k === 'success') return 'var(--ty-success)'
  if (k === 'all-failed') return 'var(--ty-danger)'
  return 'var(--ty-accent)'
})
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Completion banner -->
    <div
      v-if="store.completionBanner"
      class="flex items-center justify-between rounded-lg px-3 py-2 text-sm font-medium"
      :style="{ backgroundColor: bannerBg, color: bannerTextColor }"
    >
      <span>{{ store.completionBanner.message }}</span>
      <button
        type="button"
        class="ml-3 cursor-pointer text-lg leading-none opacity-60 hover:opacity-100"
        aria-label="关闭"
        @click="store.dismissBanner()"
      >×</button>
    </div>

    <!-- Stats row -->
    <div class="flex flex-wrap gap-3">
      <!-- Flash cumulative stats -->
      <div class="flex min-w-0 flex-1 items-center gap-4 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3">
        <BatchDonutChart
          :total="store.cumulativeStats.flash.total"
          :success="store.cumulativeStats.flash.success"
          :fail="store.cumulativeStats.flash.fail"
        />
        <div class="flex min-w-0 flex-1 flex-col gap-1 text-sm">
          <div class="flex items-center justify-between">
            <span class="font-medium text-[var(--ty-text)]">烧录累计</span>
            <button
              type="button"
              class="cursor-pointer text-xs text-[var(--ty-danger)] hover:underline"
              @click="store.resetFlashStats()"
            >重置</button>
          </div>
          <div class="flex gap-4 text-xs text-[var(--ty-text-muted)]">
            <span>总计 <strong class="text-[var(--ty-text)]">{{ store.cumulativeStats.flash.total }}</strong></span>
            <span :style="{ color: 'var(--ty-success)' }">✓ {{ store.cumulativeStats.flash.success }}</span>
            <span :style="{ color: 'var(--ty-danger)' }">✗ {{ store.cumulativeStats.flash.fail }}</span>
          </div>
        </div>
      </div>

      <!-- Auth cumulative stats (opMode includes auth) -->
      <div
        v-if="store.showAuthStats"
        class="flex min-w-0 flex-1 items-center gap-4 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
      >
        <BatchDonutChart
          :total="store.cumulativeStats.auth.total"
          :success="store.cumulativeStats.auth.success"
          :fail="store.cumulativeStats.auth.fail"
        />
        <div class="flex min-w-0 flex-1 flex-col gap-1 text-sm">
          <div class="flex items-center justify-between">
            <span class="font-medium text-[var(--ty-text)]">授权累计</span>
            <button
              type="button"
              class="cursor-pointer text-xs text-[var(--ty-danger)] hover:underline"
              @click="store.resetAuthStats()"
            >重置</button>
          </div>
          <div class="flex gap-4 text-xs text-[var(--ty-text-muted)]">
            <span>总计 <strong class="text-[var(--ty-text)]">{{ store.cumulativeStats.auth.total }}</strong></span>
            <span :style="{ color: 'var(--ty-success)' }">✓ {{ store.cumulativeStats.auth.success }}</span>
            <span :style="{ color: 'var(--ty-danger)' }">✗ {{ store.cumulativeStats.auth.fail }}</span>
          </div>
        </div>
      </div>

      <!-- Current batch stats -->
      <div class="flex min-w-0 flex-1 flex-col justify-center gap-1.5 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3 text-sm">
        <span class="font-medium text-[var(--ty-text)]">本次批次</span>
        <div class="flex flex-wrap gap-x-4 gap-y-1 text-xs">
          <span class="text-[var(--ty-primary)]">进行中 {{ store.currentStats.active }}</span>
          <span :style="{ color: 'var(--ty-success)' }">✓ 成功 {{ store.currentStats.done }}</span>
          <span :style="{ color: 'var(--ty-danger)' }">✗ 失败 {{ store.currentStats.failed }}</span>
          <span class="text-[var(--ty-text-muted)]">⏱ {{ elapsedDisplay }}</span>
        </div>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 9.2: Commit**

```bash
git add src/features/batch-flash/components/BatchFlashDashboard.vue
git commit -m "feat(batch-flash): add dashboard component"
```

---

## Task 10: BatchFlashConfig + BatchAuthConfig

**Files:**
- Create: `src/features/batch-flash/components/BatchFlashConfig.vue`
- Create: `src/features/batch-flash/components/BatchAuthConfig.vue`

- [ ] **Step 10.1: Create BatchFlashConfig.vue**

```vue
<!-- src/features/batch-flash/components/BatchFlashConfig.vue -->
<script setup lang="ts">
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri'
import { useBatchFlashStore } from '../store'
import { CHIP_IDS, BAUD_RATE_OPTIONS } from '@/features/firmware-flash/constants'
import TySelect from '@/components/TySelect.vue'

const store = useBatchFlashStore()

async function browseFirmware() {
  if (!isTauriRuntime()) return
  const { open } = await import('@tauri-apps/plugin-dialog')
  const file = await open({ filters: [{ name: 'Binary', extensions: ['bin'] }] })
  if (typeof file === 'string') store.firmwarePath = file
}
</script>

<template>
  <div class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3">
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">共享配置</h3>
    <div class="flex flex-wrap gap-3">
      <!-- Chip selector -->
      <div class="flex min-w-[9rem] flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">芯片型号</label>
        <TySelect v-model="store.chipId" :disabled="store.isBusy">
          <option v-for="id in CHIP_IDS" :key="id" :value="id">{{ id }}</option>
        </TySelect>
      </div>

      <!-- Baud rate -->
      <div class="flex min-w-[8rem] flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">波特率</label>
        <TySelect v-model.number="store.baudRate" :disabled="store.isBusy">
          <option v-for="b in BAUD_RATE_OPTIONS" :key="b" :value="b">{{ b }}</option>
        </TySelect>
      </div>

      <!-- Firmware file -->
      <div class="flex min-w-[16rem] flex-1 flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">
          固件文件
          <span v-if="store.opMode !== 'auth-only'" class="text-[var(--ty-danger)]">*</span>
        </label>
        <div class="flex gap-2">
          <input
            type="text"
            :value="store.firmwarePath"
            readonly
            :disabled="store.isBusy"
            placeholder="未选择文件"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 py-1.5 text-xs text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
          />
          <button
            type="button"
            class="ops-browse-btn"
            :disabled="store.isBusy"
            @click="browseFirmware"
          >浏览</button>
        </div>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 10.2: Create BatchAuthConfig.vue** (stub for Phase 1 — full auth wiring in Phase 2)

```vue
<!-- src/features/batch-flash/components/BatchAuthConfig.vue -->
<script setup lang="ts">
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri'
import { useBatchFlashStore } from '../store'

const store = useBatchFlashStore()

async function browseExcel() {
  if (!isTauriRuntime()) return
  const { open } = await import('@tauri-apps/plugin-dialog')
  const file = await open({ filters: [{ name: 'Excel', extensions: ['xlsx'] }] })
  if (typeof file === 'string') store.authConfig.excelPath = file
}
</script>

<template>
  <div
    class="rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] px-4 py-3"
    style="border-left: 3px solid var(--ty-accent);"
  >
    <h3 class="mb-3 text-sm font-semibold text-[var(--ty-text)]">批量授权配置</h3>
    <div class="flex flex-col gap-3">
      <!-- Excel file -->
      <div class="flex flex-col gap-1">
        <label class="text-xs text-[var(--ty-text-muted)]">授权表 (.xlsx)</label>
        <div class="flex gap-2">
          <input
            type="text"
            :value="store.authConfig.excelPath"
            readonly
            :disabled="store.isBusy"
            placeholder="未选择授权表"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 py-1.5 text-xs text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
          />
          <button
            type="button"
            class="ops-browse-btn"
            :disabled="store.isBusy"
            @click="browseExcel"
          >浏览</button>
        </div>
      </div>

      <!-- Conflict policy -->
      <div class="flex items-center gap-4 text-xs text-[var(--ty-text-muted)]">
        <span>遇到已授权设备：</span>
        <label class="flex cursor-pointer items-center gap-1">
          <input type="radio" v-model="store.authConfig.conflictPolicy" value="skip" :disabled="store.isBusy" />
          跳过（推荐）
        </label>
        <label class="flex cursor-pointer items-center gap-1">
          <input type="radio" v-model="store.authConfig.conflictPolicy" value="overwrite" :disabled="store.isBusy" />
          覆盖
        </label>
      </div>
    </div>
  </div>
</template>
```

- [ ] **Step 10.3: Commit**

```bash
git add src/features/batch-flash/components/BatchFlashConfig.vue \
        src/features/batch-flash/components/BatchAuthConfig.vue
git commit -m "feat(batch-flash): add config components"
```

---

## Task 11: PortFilterModal

**Files:**
- Create: `src/features/batch-flash/components/PortFilterModal.vue`

- [ ] **Step 11.1: Create PortFilterModal.vue**

```vue
<!-- src/features/batch-flash/components/PortFilterModal.vue -->
<script setup lang="ts">
import { ref } from 'vue'
import { useBatchFlashStore } from '../store'
import TyConfirmDialog from '@/components/TyConfirmDialog.vue'

defineProps<{ open: boolean }>()
const emit = defineEmits<{ close: [] }>()
const store = useBatchFlashStore()
const newPort = ref('')

function addPort() {
  const p = newPort.value.trim()
  if (p) {
    store.addBlockedPort(p)
    newPort.value = ''
  }
}
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      @click.self="$emit('close')"
    >
      <div
        class="w-full max-w-md rounded-2xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-5 shadow-xl"
      >
        <div class="mb-4 flex items-center justify-between">
          <h2 class="text-base font-semibold text-[var(--ty-text)]">串口过滤</h2>
          <button
            type="button"
            class="cursor-pointer text-xl text-[var(--ty-text-muted)] hover:text-[var(--ty-text)]"
            aria-label="关闭"
            @click="$emit('close')"
          >×</button>
        </div>

        <p class="mb-3 text-xs text-[var(--ty-text-muted)]">
          添加要屏蔽的串口名称（精确匹配，Windows 不区分大小写）。
          规则生效后，自动分配时将跳过这些串口。
        </p>

        <!-- Add port input -->
        <div class="mb-4 flex gap-2">
          <input
            v-model="newPort"
            type="text"
            placeholder="如 COM1 或 /dev/ttyS0"
            class="min-w-0 flex-1 rounded-lg border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] px-2.5 py-1.5 text-sm text-[var(--ty-text)] placeholder:text-[var(--ty-text-muted)]"
            @keydown.enter="addPort"
          />
          <button type="button" class="ty-btn-secondary px-3 text-sm" @click="addPort">添加</button>
        </div>

        <!-- Blocked ports list -->
        <div v-if="store.filterConfig.blockedPorts.length > 0" class="flex flex-col gap-1">
          <div
            v-for="port in store.filterConfig.blockedPorts"
            :key="port"
            class="flex items-center justify-between rounded-lg bg-[var(--ty-surface-muted)] px-3 py-1.5"
          >
            <span class="font-mono text-xs text-[var(--ty-text)]">{{ port }}</span>
            <button
              type="button"
              class="cursor-pointer text-sm text-[var(--ty-text-muted)] hover:text-[var(--ty-danger)]"
              aria-label="移除"
              @click="store.removeBlockedPort(port)"
            >×</button>
          </div>
        </div>
        <p v-else class="text-xs text-[var(--ty-text-muted)]">暂无过滤规则</p>
      </div>
    </div>
  </Teleport>
</template>
```

- [ ] **Step 11.2: Commit**

```bash
git add src/features/batch-flash/components/PortFilterModal.vue
git commit -m "feat(batch-flash): add port filter modal"
```

---

## Task 12: BatchFlashToolbar

**Files:**
- Create: `src/features/batch-flash/components/BatchFlashToolbar.vue`

- [ ] **Step 12.1: Create BatchFlashToolbar.vue**

```vue
<!-- src/features/batch-flash/components/BatchFlashToolbar.vue -->
<script setup lang="ts">
import { ref } from 'vue'
import { useBatchFlashStore } from '../store'
import PortFilterModal from './PortFilterModal.vue'
import { showConfirmDialog } from '@/composables/confirmDialog'

const store = useBatchFlashStore()
const filterOpen = ref(false)

async function handleStart() {
  if (store.slots.filter(s => s.status === 'idle').length > 8) {
    const ok = await showConfirmDialog({
      title: '确认批量烧录',
      message: `即将对 ${store.slots.filter(s => s.status === 'idle').length} 个端口并行烧录\n固件：${store.firmwarePath}`,
      kind: 'warning',
    })
    if (!ok) return
  }
  await store.startFlash()
}

async function handleAutoAssign() {
  await store.autoAssign()
}
</script>

<template>
  <div class="flex items-center gap-2">
    <!-- Left: functional controls -->
    <div class="flex items-center gap-2">
      <button
        type="button"
        class="ty-btn-secondary flex items-center gap-1.5 text-sm"
        :disabled="store.isBusy"
        :title="store.isBusy ? '任务进行中，不可自动分配' : '扫描并添加可用串口'"
        @click="handleAutoAssign"
      >
        <FontAwesomeIcon :icon="['fas', 'rotate']" class="size-3.5" />
        自动分配
      </button>

      <button
        type="button"
        class="ty-btn-secondary relative flex items-center gap-1.5 text-sm"
        @click="filterOpen = true"
      >
        <FontAwesomeIcon :icon="['fas', 'filter']" class="size-3.5" />
        串口过滤
        <span
          v-if="store.filterActive"
          class="absolute -right-1.5 -top-1.5 flex h-4 w-4 items-center justify-center rounded-full text-[10px] font-bold"
          :style="{ backgroundColor: 'var(--ty-accent)', color: '#fff' }"
        >{{ store.filterConfig.blockedPorts.length }}</span>
      </button>
    </div>

    <div class="flex-1" />

    <!-- Right: action buttons -->
    <div class="flex items-center gap-2">
      <button
        type="button"
        class="ty-btn-secondary text-sm"
        :disabled="!store.canCancel"
        @click="store.cancelAll()"
      >取消</button>

      <button
        type="button"
        class="ty-btn-secondary text-sm"
        :disabled="!store.canRetry"
        :title="!store.canRetry ? '暂无失败端口' : undefined"
        @click="store.retryFailed()"
      >重试失败</button>

      <button
        type="button"
        class="ty-btn-primary-solid text-sm"
        :disabled="!store.canStart"
        :title="
          !store.firmwarePath && store.opMode !== 'auth-only' ? '请先选择固件文件' :
          !store.slots.some(s => s.status === 'idle') ? '暂无空闲串口' : undefined
        "
        @click="handleStart"
      >
        <FontAwesomeIcon :icon="['fas', 'play']" class="mr-1 size-3" />
        全部开始
      </button>
    </div>
  </div>

  <PortFilterModal :open="filterOpen" @close="filterOpen = false" />
</template>
```

- [ ] **Step 12.2: Commit**

```bash
git add src/features/batch-flash/components/BatchFlashToolbar.vue
git commit -m "feat(batch-flash): add toolbar component"
```

---

## Task 13: BatchFlashPage – final assembly

**Files:**
- Modify: `src/features/batch-flash/BatchFlashPage.vue`

- [ ] **Step 13.1: Replace stub with full page**

```vue
<!-- src/features/batch-flash/BatchFlashPage.vue -->
<script setup lang="ts">
import { onMounted, onUnmounted } from 'vue'
import { useBatchFlashStore } from './store'
import BatchFlashDashboard from './components/BatchFlashDashboard.vue'
import BatchFlashConfig from './components/BatchFlashConfig.vue'
import BatchAuthConfig from './components/BatchAuthConfig.vue'
import BatchFlashToolbar from './components/BatchFlashToolbar.vue'
import BatchFlashSlotList from './components/BatchFlashSlotList.vue'

const store = useBatchFlashStore()

onMounted(async () => {
  await store.loadPersistedData()
  await store.ensureListener()
})

onUnmounted(() => {
  if (!store.isBusy) store.cleanup()
})
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col gap-3">
    <!-- Page title -->
    <div>
      <h1 class="text-lg font-semibold text-[var(--ty-text)]">批量烧录</h1>
      <p class="text-xs text-[var(--ty-text-muted)]">同时向多个串口并行烧录同一份固件，最多 32 个</p>
    </div>

    <!-- Dashboard -->
    <BatchFlashDashboard />

    <!-- Shared config -->
    <BatchFlashConfig />

    <!-- Auth config (ESP32/GD32 only) -->
    <BatchAuthConfig v-if="store.authSupported" />

    <!-- Toolbar -->
    <BatchFlashToolbar />

    <!-- Slot list -->
    <BatchFlashSlotList class="min-h-0 flex-1" />
  </div>
</template>
```

- [ ] **Step 13.2: Commit**

```bash
git add src/features/batch-flash/BatchFlashPage.vue
git commit -m "feat(batch-flash): assemble BatchFlashPage"
```

---

## Task 14: i18n strings

**Files:**
- Modify: `src/locales/zh-CN.json`
- Modify: `src/locales/en.json`

- [ ] **Step 14.1: Add zh-CN keys** (inside the root JSON object)

```json
"batchFlash": {
  "title": "批量烧录",
  "subtitle": "同时向多个串口并行烧录同一份固件，最多 32 个",
  "dashboard": {
    "flashStats": "烧录累计",
    "authStats": "授权累计",
    "currentBatch": "本次批次",
    "active": "进行中",
    "success": "成功",
    "fail": "失败",
    "reset": "重置",
    "elapsed": "已用时"
  },
  "config": {
    "chip": "芯片型号",
    "baud": "波特率",
    "firmware": "固件文件",
    "browse": "浏览",
    "noFile": "未选择文件",
    "authTitle": "批量授权配置",
    "excelFile": "授权表 (.xlsx)",
    "noExcel": "未选择授权表",
    "conflictPolicy": "遇到已授权设备",
    "skip": "跳过（推荐）",
    "overwrite": "覆盖"
  },
  "toolbar": {
    "autoAssign": "自动分配",
    "filter": "串口过滤",
    "start": "全部开始",
    "cancel": "取消",
    "retry": "重试失败"
  },
  "slot": {
    "port": "串口",
    "status": "状态",
    "progress": "进度",
    "action": "操作",
    "cancel": "取消",
    "retry": "重试",
    "remove": "删除",
    "empty": "暂无串口",
    "emptyHint": "点击「自动分配」扫描可用串口，或手动添加"
  },
  "status": {
    "idle": "空闲",
    "flashing": "烧录中",
    "reading_mac": "读取MAC",
    "authorizing": "写入授权",
    "done": "成功",
    "failed": "失败",
    "skipped": "已跳过"
  }
}
```

- [ ] **Step 14.2: Add en keys**

```json
"batchFlash": {
  "title": "Batch Flash",
  "subtitle": "Flash the same firmware to up to 32 ports in parallel",
  "dashboard": {
    "flashStats": "Flash Stats",
    "authStats": "Auth Stats",
    "currentBatch": "Current Batch",
    "active": "Active",
    "success": "Success",
    "fail": "Failed",
    "reset": "Reset",
    "elapsed": "Elapsed"
  },
  "config": {
    "chip": "Chip",
    "baud": "Baud Rate",
    "firmware": "Firmware File",
    "browse": "Browse",
    "noFile": "No file selected",
    "authTitle": "Batch Auth Config",
    "excelFile": "Auth Table (.xlsx)",
    "noExcel": "No auth table selected",
    "conflictPolicy": "Already-authorized device",
    "skip": "Skip (recommended)",
    "overwrite": "Overwrite"
  },
  "toolbar": {
    "autoAssign": "Auto Assign",
    "filter": "Port Filter",
    "start": "Start All",
    "cancel": "Cancel",
    "retry": "Retry Failed"
  },
  "slot": {
    "port": "Port",
    "status": "Status",
    "progress": "Progress",
    "action": "Action",
    "cancel": "Cancel",
    "retry": "Retry",
    "remove": "Remove",
    "empty": "No ports",
    "emptyHint": "Click Auto Assign to scan available ports"
  },
  "status": {
    "idle": "Idle",
    "flashing": "Flashing",
    "reading_mac": "Reading MAC",
    "authorizing": "Writing Auth",
    "done": "Done",
    "failed": "Failed",
    "skipped": "Skipped"
  }
}
```

- [ ] **Step 14.3: Final build verification**

```bash
pnpm run build
```

Expected: zero TypeScript errors, build completes successfully.

- [ ] **Step 14.4: Run all batch-flash tests**

```bash
pnpm exec vitest run src/features/batch-flash/
```

Expected: all tests PASS.

- [ ] **Step 14.5: Commit**

```bash
git add src/locales/zh-CN.json src/locales/en.json
git commit -m "feat(batch-flash): add i18n strings for batch flash page"
```

---

## Phase 1 complete ✓

Phase 1 delivers a fully functional batch flash page. The `BatchAuthConfig` component is wired for UI (file picker, conflict policy) but auth execution is gated in Phase 2.

**Phase 2 plan** (`2026-06-01-batch-flash-phase2.md`) will cover:
- Rust: `calamine` (xlsx read) + `rust_xlsxwriter` (xlsx write) dependencies
- `ExcelRowAllocator` with atomic row allocation
- `batch_auth_start` / `batch_auth_cancel_port` / `batch_auth_cancel_all` Tauri commands
- `batch-auth-progress` event + store handler
- Auth slot state machine wiring (`reading_mac → authorizing → done/failed/skipped`)
- Auth cumulative stats increment
- Session log generation (`表名_auth_时间戳.log`)
