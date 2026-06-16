# Batch Auth Mandatory / Flash Optional Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Excel auth table mandatory and firmware optional in the batch flash auth tool, removing `flash-only` mode and restricting the chip selector to `esp32` and `other`.

**Architecture:** Pure frontend change. Types first (constants + `BatchOpMode`), then store logic, then Vue components. No Rust backend changes needed — `batch_auth_start` already handles both `auth-only` and `flash-then-auth`; cancel commands already share the same flag.

**Tech Stack:** TypeScript, Vue 3, Pinia. Tests run with Vitest (`pnpm run test`).

---

## File Map

| File | Change |
|---|---|
| `src/features/batch-flash-auth/types.ts` | Remove `BATCH_AUTH_SUPPORTED_CHIPS`; add `BATCH_AUTH_TOOL_CHIP_OPTIONS`, `BATCH_FLASH_CAPABLE_CHIPS`; narrow `BatchOpMode` |
| `src/stores/batch-flash-auth.ts` | Default chipId, remove `authSupported`, add `canFlash`, update 4 computeds + 3 actions + return block |
| `src/stores/batch-flash-auth.test.ts` | Delete stale tests; add `canFlash` / `inputsValid` / updated `opMode` tests |
| `src/features/batch-flash-auth/BatchFlashAuthPage.vue` | Remove `v-if="store.authSupported"` |
| `src/features/batch-flash-auth/components/BatchFlashAuthConfig.vue` | Chip options, firmware `v-if`, remove asterisk |
| `src/features/batch-flash-auth/components/BatchFlashAuthToolbar.vue` | Remove 3 stale `opMode` references |

---

## Task 1: Update types.ts

**Files:**
- Modify: `src/features/batch-flash-auth/types.ts`

- [ ] **Step 1: Apply the type changes**

Replace the existing `BATCH_AUTH_SUPPORTED_CHIPS` export and `BatchOpMode` with:

```ts
// Remove this line entirely:
// export const BATCH_AUTH_SUPPORTED_CHIPS = ["esp32"] as const;

// Add after the existing imports/comments:

/** Chips available in the batch auth tool (all support the auth serial protocol). */
// When GD32 support is added to the Rust plugin registry, append "gd32" here.
export const BATCH_AUTH_TOOL_CHIP_OPTIONS = ["esp32", "other"] as const;

/** Subset of BATCH_AUTH_TOOL_CHIP_OPTIONS that also have a registered flash plugin. */
// When GD32 support is added to the Rust plugin registry, append "gd32" here.
export const BATCH_FLASH_CAPABLE_CHIPS = ["esp32"] as const;
```

Change `BatchOpMode` (remove `"flash-only"`):

```ts
// Before:
export type BatchOpMode = "flash-only" | "auth-only" | "flash-then-auth";

// After:
export type BatchOpMode = "auth-only" | "flash-then-auth";
```

- [ ] **Step 2: Verify TypeScript compiles**

```bash
pnpm run build
```

Expected: type errors about `"flash-only"` in `batch-flash-auth.ts` and test file (these are expected — will be fixed in Tasks 2–3).

---

## Task 2: Write failing store tests

**Files:**
- Modify: `src/stores/batch-flash-auth.test.ts`

- [ ] **Step 1: Delete stale `opMode` tests (lines 13–29)**

Remove these three `it(...)` blocks from the `describe("opMode")` block:

```ts
// DELETE these three tests:
it("is flash-only when no firmware and no excel", () => { ... });
it("is flash-only when firmware selected but no excel", () => { ... });
it("is flash-only when excel selected but chip does not support auth", () => { ... });
```

The two remaining `opMode` tests (`auth-only` and `flash-then-auth`) stay as-is.

- [ ] **Step 2: Add two new `opMode` tests**

Inside `describe("opMode")`, append:

```ts
it("is auth-only by default (chip=esp32, no firmware, no excel)", () => {
  const store = useBatchFlashAuthStore();
  expect(store.opMode).toBe("auth-only");
});

it("is auth-only when chip=other regardless of firmware and excel", () => {
  const store = useBatchFlashAuthStore();
  store.chipId = "other";
  store.firmwarePath = "/fw.bin";
  store.authConfig.excelPath = "/auth.xlsx";
  expect(store.opMode).toBe("auth-only");
});
```

- [ ] **Step 3: Delete the entire `describe("authSupported")` block (lines 47–67)**

Remove:

```ts
describe("authSupported", () => {
  beforeEach(() => setActivePinia(createPinia()));
  it("is true for esp32", ...);
  it("is false for gd32 ...", ...);
  it("is false for bk7231n", ...);
});
```

- [ ] **Step 4: Add `describe("canFlash")` block**

Add after the `describe("opMode")` closing brace:

```ts
describe("canFlash", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is true for esp32", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    expect(store.canFlash).toBe(true);
  });

  it("is false for other", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "other";
    expect(store.canFlash).toBe(false);
  });
});
```

- [ ] **Step 5: Add `describe("inputsValid")` block**

Add after `describe("canFlash")`:

```ts
describe("inputsValid", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is false with no inputs", () => {
    const store = useBatchFlashAuthStore();
    expect(store.inputsValid).toBe(false);
  });

  it("is false when firmware set but no excel", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.firmwarePath = "/fw.bin";
    expect(store.inputsValid).toBe(false);
  });

  it("is true when excel set, no firmware", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "esp32";
    store.authConfig.excelPath = "/auth.xlsx";
    expect(store.inputsValid).toBe(true);
  });

  it("is true when chip=other with excel (firmware ignored)", () => {
    const store = useBatchFlashAuthStore();
    store.chipId = "other";
    store.firmwarePath = "/fw.bin";
    store.authConfig.excelPath = "/auth.xlsx";
    expect(store.inputsValid).toBe(true);
  });
});
```

- [ ] **Step 6: Delete stale `canStart` test (lines 171–175)**

Remove this test:

```ts
// DELETE:
it("canStart is false when firmware missing (flash-only mode)", () => {
  const store = useBatchFlashAuthStore();
  store.addPorts(["COM3"]);
  expect(store.canStart).toBe(false);
});
```

- [ ] **Step 7: Fix the `canStart is true` test (line 177–182)**

The existing test sets only `firmwarePath` but no excel — it passes today because flash-only mode doesn't require excel. After the change `inputsValid` will require excel, so this test will fail. Replace it:

```ts
// Before:
it("canStart is true when idle slot exists and inputs valid", () => {
  const store = useBatchFlashAuthStore();
  store.addPorts(["COM3"]);
  store.firmwarePath = "/fw.bin";
  expect(store.canStart).toBe(true);
});

// After:
it("canStart is true when idle slot exists and excel is set", () => {
  const store = useBatchFlashAuthStore();
  store.addPorts(["COM3"]);
  store.chipId = "esp32";
  store.authConfig.excelPath = "/auth.xlsx";
  expect(store.canStart).toBe(true);
});
```

- [ ] **Step 8: Run tests — expect failures**

```bash
pnpm run test
```

Expected: failures on `canFlash`, `inputsValid` new tests, and TypeScript errors on `authSupported`. The existing passing tests should still pass (slot state machine, completionBanner, etc.).

---

## Task 3: Update the Pinia store

**Files:**
- Modify: `src/stores/batch-flash-auth.ts`

- [ ] **Step 1: Update imports from types.ts**

Replace:

```ts
import {
  BATCH_AUTH_SUPPORTED_CHIPS,
  type BatchSlotState,
  // ...
} from "@/features/batch-flash-auth/types";
```

With:

```ts
import {
  BATCH_AUTH_TOOL_CHIP_OPTIONS,
  BATCH_FLASH_CAPABLE_CHIPS,
  type BatchSlotState,
  // ...
} from "@/features/batch-flash-auth/types";
```

Also remove the import of `DEFAULT_CHIP_ID` from `@/features/firmware-flash/constants` if it is only used for the initial `chipId` value (check: if it's used elsewhere in the store, keep it).

- [ ] **Step 2: Change the default `chipId`**

```ts
// Before:
const chipId = ref<string>(DEFAULT_CHIP_ID);

// After (chipId is not persisted; changing the default is sufficient):
const chipId = ref<string>("esp32");
```

- [ ] **Step 3: Remove `authSupported`, add `canFlash`**

```ts
// Remove:
const authSupported = computed(() =>
  (BATCH_AUTH_SUPPORTED_CHIPS as readonly string[]).includes(chipId.value),
);

// Add:
const canFlash = computed(() =>
  (BATCH_FLASH_CAPABLE_CHIPS as readonly string[]).includes(chipId.value),
);
```

- [ ] **Step 4: Update `opMode`**

```ts
// Before:
const opMode = computed<BatchOpMode>(() => {
  const hasFirmware = !!firmwarePath.value;
  const hasExcel = authSupported.value && !!authConfig.value.excelPath;
  if (hasFirmware && hasExcel) return "flash-then-auth";
  if (hasExcel) return "auth-only";
  return "flash-only";
});

// After:
const opMode = computed<BatchOpMode>(() =>
  canFlash.value && !!firmwarePath.value ? "flash-then-auth" : "auth-only",
);
```

- [ ] **Step 5: Update `inputsValid`**

```ts
// Before:
const inputsValid = computed(() => {
  if (opMode.value !== "auth-only" && !firmwarePath.value) return false;
  if (opMode.value !== "flash-only" && !authConfig.value.excelPath)
    return false;
  return true;
});

// After (excel always required; firmware never required):
const inputsValid = computed(() => !!authConfig.value.excelPath);
```

- [ ] **Step 6: Update `showAuthStats`**

```ts
// Before:
const showAuthStats = computed(() => opMode.value !== "flash-only");

// After (auth stats always shown in this tool):
const showAuthStats = computed(() => true);
```

- [ ] **Step 7: Update `startBatch`**

```ts
// Before:
async function startBatch() {
  if (opMode.value === "flash-only") {
    await startFlash();
  } else {
    await startAuth();
  }
}

// After:
async function startBatch() {
  await startAuth();
}
```

- [ ] **Step 8: Update `cancelPort` and `cancelAll`**

```ts
// Before (both have flash-only ternary):
async function cancelPort(port: string) {
  if (!isTauriRuntime()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  const cmd =
    opMode.value === "flash-only"
      ? "batch_flash_cancel_port"
      : "batch_auth_cancel_port";
  await invoke(cmd, { port });
}

async function cancelAll() {
  if (!isTauriRuntime()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  const cmd =
    opMode.value === "flash-only"
      ? "batch_flash_cancel_all"
      : "batch_auth_cancel_all";
  await invoke(cmd);
}

// After (flash-then-auth is managed by batch_auth_start, cancel is shared):
async function cancelPort(port: string) {
  if (!isTauriRuntime()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("batch_auth_cancel_port", { port });
}

async function cancelAll() {
  if (!isTauriRuntime()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("batch_auth_cancel_all");
}
```

- [ ] **Step 9: Update the store `return` block**

```ts
// Remove from return:
authSupported,

// Add to return:
canFlash,
```

- [ ] **Step 10: Run tests — all should pass**

```bash
pnpm run test
```

Expected: all tests pass, including the new `canFlash` and `inputsValid` describe blocks.

- [ ] **Step 11: Commit**

```bash
git add src/features/batch-flash-auth/types.ts src/stores/batch-flash-auth.ts src/stores/batch-flash-auth.test.ts
git commit -m "refactor(batch-auth): make auth mandatory, flash optional

Remove flash-only mode. Excel auth table is now always required.
Firmware is optional (only available for flash-capable chips).
Add canFlash computed; restrict chip list to esp32 + other."
```

---

## Task 4: Update Vue components

**Files:**
- Modify: `src/features/batch-flash-auth/BatchFlashAuthPage.vue`
- Modify: `src/features/batch-flash-auth/components/BatchFlashAuthConfig.vue`
- Modify: `src/features/batch-flash-auth/components/BatchFlashAuthToolbar.vue`

- [ ] **Step 1: BatchFlashAuthPage.vue — remove `v-if`**

```html
<!-- Before: -->
<BatchAuthConfig v-if="store.authSupported" />

<!-- After: -->
<BatchAuthConfig />
```

- [ ] **Step 2: BatchFlashAuthConfig.vue — update chip options**

In the `<script setup>` section, change the chip options import and computation:

```ts
// Add to imports from types:
import {
  BATCH_AUTH_TOOL_CHIP_OPTIONS,
} from "../types";

// Remove:
import {
  CHIP_IDS,
  BAUD_RATE_OPTIONS,
} from "@/features/firmware-flash/constants";
// Keep BAUD_RATE_OPTIONS; only CHIP_IDS is being replaced.

// Replace chipOptions computed:
// Before:
const chipOptions = computed<TySelectOption[]>(() =>
  (CHIP_IDS as readonly string[]).map((id) => ({
    value: id,
    label: t(`flash.chips.${id}`),
  })),
);

// After:
const chipOptions = computed<TySelectOption[]>(() =>
  (BATCH_AUTH_TOOL_CHIP_OPTIONS as readonly string[]).map((id) => ({
    value: id,
    label: t(`flash.chips.${id}`),
  })),
);
```

- [ ] **Step 3: BatchFlashAuthConfig.vue — wrap firmware section with `v-if`**

The firmware `<div>` currently looks like:

```html
<!-- Firmware file -->
<div class="flex min-w-[16rem] flex-1 flex-col gap-1">
  <label class="text-xs text-[var(--ty-text-muted)]">
    固件文件
    <span
      v-if="store.opMode !== 'auth-only'"
      class="text-[var(--ty-danger)]"
      >*</span
    >
  </label>
  <div class="flex gap-2">
    ...
  </div>
</div>
```

Replace with (outer `v-if`, remove the asterisk `<span>`):

```html
<!-- Firmware file — only shown for flash-capable chips (not "other") -->
<div v-if="store.canFlash" class="flex min-w-[16rem] flex-1 flex-col gap-1">
  <label class="text-xs text-[var(--ty-text-muted)]">固件文件</label>
  <div class="flex gap-2">
    ...
  </div>
</div>
```

- [ ] **Step 4: BatchFlashAuthToolbar.vue — fix stale `opMode` references**

**Line 17** — `excelInfo` in the bulk-confirm dialog:

```ts
// Before:
const excelInfo =
  store.opMode !== "flash-only" && store.authConfig.excelPath
    ? `\n授权表：${store.authConfig.excelPath}`
    : "";

// After (opMode is never flash-only; excel always present when we reach this dialog):
const excelInfo = store.authConfig.excelPath
  ? `\n授权表：${store.authConfig.excelPath}`
  : "";
```

**Line 21** — `firmwareInfo` in the bulk-confirm dialog:

```ts
// Before:
const firmwareInfo =
  store.opMode !== "auth-only" && store.firmwarePath
    ? `\n固件：${store.firmwarePath}`
    : "";

// After (explicit is clearer):
const firmwareInfo =
  store.opMode === "flash-then-auth" && store.firmwarePath
    ? `\n固件：${store.firmwarePath}`
    : "";
```

**Line 100** — start button `:title` tooltip:

```html
<!-- Before: -->
:title="
  !store.firmwarePath && store.opMode !== 'auth-only'
    ? '请先选择固件文件'
    : !store.slots.some((s) => s.status === 'idle')
      ? '暂无空闲串口'
      : undefined
"

<!-- After (firmware never required; excel is the blocking input): -->
:title="
  !store.authConfig.excelPath
    ? '请先选择授权表'
    : !store.slots.some((s) => s.status === 'idle')
      ? '暂无空闲串口'
      : undefined
"
```

- [ ] **Step 5: Verify build**

```bash
pnpm run build
```

Expected: no TypeScript errors.

- [ ] **Step 6: Run full test suite**

```bash
pnpm run test
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/features/batch-flash-auth/BatchFlashAuthPage.vue \
        src/features/batch-flash-auth/components/BatchFlashAuthConfig.vue \
        src/features/batch-flash-auth/components/BatchFlashAuthToolbar.vue
git commit -m "feat(batch-auth): hide firmware for other chip, auth config always visible

BatchAuthConfig no longer gated by authSupported — always shown.
Firmware section hidden when chip=other (canFlash=false).
Toolbar tooltips updated: excel required, firmware never required."
```
