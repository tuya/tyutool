# Frontend Conventions (src/)

## Vue components

- Shared components go in `src/components/`, prefixed with `Ty` (`TySelect`, `TyToast`)
- Feature-internal components go in `features/<name>/components/` (when that subdirectory exists), no prefix
- Feature structure varies by size: `serial-debug/` has a `components/` subdirectory; `settings/` places non-page components directly in the feature root

## Pinia stores

- All stores use the setup store style (`defineStore('name', () => { ... })`)
- Stores manage cross-component shared state only; single-component state stays in the component
- Side effects (Tauri calls, event listeners) belong in store actions, not scattered across components
- Workspace persistence uses a separate utility module (`*-workspace.ts`) that exports async load/save functions — not a Pinia store
- `usePortManagerStore` is the cross-feature serial port ownership coordinator; any new serial port feature must go through it

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

## UI parity: web vs Tauri

- The UI must look and behave identically in `dev:web` and `tauri:dev`; any visible difference is a bug.
- `TySelect` displays `placeholder` (default `'—'`) when no option matches `modelValue`. If you render a disabled placeholder option with `value: ''`, also bind `:placeholder` to the same text — otherwise the button shows `'—'` whenever `modelValue` is a non-empty string that doesn't match any real option.
