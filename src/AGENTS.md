# Frontend Conventions (src/)

## Directory layout

| Path | Purpose |
|------|---------|
| `app-init.ts` | `bootstrapApp()` — workspace restore after settings ready |
| `runtime.ts` | `isTauriRuntime()`, `getRuntime()` |
| `platform.ts` | File picker + WS URL platform abstraction |
| `transport/` | Shared WebSocket client (`ws-transport.ts`) |
| `features/<name>/` | Feature UI; Pinia stores live in `stores/` |
| `stores/` | All Pinia stores + `*-workspace.ts` persistence modules |
| `components/` | Cross-feature `Ty*` components and `AppShell.vue` |
| `config/` | Static constants (`app-identifier.ts`, `app-nav.ts`, …) |
| `composables/` | Global `use*` composables or singleton reactive modules |

**Pinia stores** belong in `stores/` (including `batch-flash-auth`). **Workspace persistence** uses `stores/<feature>-workspace.ts`, not inline store logic.

**Toolbox tools:** `features/toolbox/` is the landing-page shell (`ToolboxPage.vue`, `tools.ts`). Each tool is a sibling feature directory (e.g. `features/batch-flash-auth/`) with route `/toolbox/<tool-id>`. Sub-tool pages use `features/toolbox/components/ToolboxBreadcrumb.vue`.

**Runtime detection:** import `isTauriRuntime` from `@/runtime`, not from feature modules.

**IPC types:** flash progress/job payloads are in `features/firmware-flash/flash-ipc-types.ts` (mirrors Rust types).

## Vue components

- Shared components go in `src/components/`, prefixed with `Ty` (`TySelect`, `TyToast`)
- Feature-internal components go in `features/<name>/components/` (when that subdirectory exists), no prefix
- Feature structure varies by size: `serial-debug/` has a `components/` subdirectory; `settings/` places non-page components directly in the feature root
- Feature-internal components use a **feature prefix** when multiple components exist (e.g. `BatchFlashAuthToolbar.vue`, `SerialDebugLogView.vue`)

## Pinia stores

- All stores use the setup store style (`defineStore('name', () => { ... })`)
- Stores manage cross-component shared state only; single-component state stays in the component
- Side effects (Tauri calls, event listeners) belong in store actions, not scattered across components
- Workspace persistence uses a separate utility module (`*-workspace.ts`) that exports async load/save functions — not a Pinia store
- `usePortManagerStore` is the cross-feature serial port ownership coordinator; any new serial port feature must go through it
- **Large setup stores are decomposed into composables, not split into more stores.** `useFlashStore` is a façade: cohesive internal subsystems live in `features/firmware-flash/useFlash*.ts` composables (`useFlashLog`, `useFlashProgress`, `useFlashConnection`) and pure helpers (`validate-operation.ts`, `browser-download.ts`). The store calls each composable and **destructures its refs/functions back into local bindings**, so the public API (the `return {}` block) and all call sites stay unchanged. Cross-subsystem effects are passed in as a `deps` object (injected refs + callbacks) — e.g. the progress composable receives `onOperationSettled`, the connection composable receives `onCancelRunningOperation` — rather than reaching across boundaries. The orchestrator (`startOperation`) and the workspace serializer deliberately stay in the store: they coordinate ~all state, so extracting them would only create leaky wide-interface modules.

## Shared state vs. composables

- **Composable** (reusable logic): filename and exported function prefixed with `use` (`useAutoUpdate.ts`); lives in `src/composables/` or within the feature directory
- **Shared reactive state module** (singleton state): exports a `reactive()` object and helper functions directly, no `use` prefix (existing examples: `confirmDialog.ts`, `toastState.ts`)

## TypeScript types

- Each feature's domain types are consolidated in `features/<name>/types.ts`
- Types that mirror Rust backend types must include a comment indicating the mirror relationship (`// Mirrors tyutool_core::serial_debug::DebugConfig`)

## i18n

- All UI strings must go through `t()`; no hardcoded text
- Key structure: `rootKey.section.item`, all camelCase; root keys are camelCase feature names (`serialDebug`, `flash`, `settings`, `app`, `common`) — not directory names
- New keys must be added to both `zh-CN.json` and `en.json` simultaneously
- `i18n/index.test.ts` guards this: it asserts en/zh key-set parity **and** that every literal `t('…')`/`t("…")` key in source is defined in `en.json` (a missing key surfaces the raw key string to users at runtime). Dynamic template-literal keys (`` t(`flash.chips.${id}`) ``) are not statically checkable, so every dynamic key family must have a static test that exercises its concrete values — otherwise a missing one slips past both guards.

## UI parity: web vs Tauri

- The UI must look and behave identically in `dev:web` and `tauri:dev`; any visible difference is a bug.
- `TySelect` displays `placeholder` (default `'—'`) when no option matches `modelValue`. If you render a disabled placeholder option with `value: ''`, also bind `:placeholder` to the same text — otherwise the button shows `'—'` whenever `modelValue` is a non-empty string that doesn't match any real option.
