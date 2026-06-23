# 批量烧录授权：授权必填 / 烧录可选 设计文档

**日期**: 2026-06-15
**分支**: `yj/toolbox`
**状态**: 待实现

---

## 背景与目标

当前「批量烧录授权」工具的 `opMode` 包含三种模式：`flash-only | auth-only | flash-then-auth`。
Excel 授权表是可选项，不填时退化为"仅烧录"——这与工具定位（**授权**工具）矛盾。

**目标**：
- 授权（Excel 授权表）改为**必填项**
- 烧录（固件文件）改为**可选项**
- 删除 `flash-only` 模式
- 芯片选项收窄为本工具实际支持的范围：`esp32`、`other`（GD32 加入时追加）

---

## 芯片约束

| 芯片 | 支持授权 | 支持烧录 |
|---|---|---|
| ESP32 | ✓ | ✓ |
| GD32（待接入） | ✓ | ✓ |
| 通用设备（other） | ✓ | ✗（故意限制，非技术限制） |

- 授权协议是通用串口通信，理论上任何设备均可，但工具界面有意只暴露上述芯片。
- 烧录需要注册的 flash plugin，仅 ESP32/GD32 有。
- 选"通用设备"时固件区域**直接隐藏**（不是禁用），`opMode` 始终为 `auth-only`。

---

## 操作模式

| 芯片 | 固件 | Excel | opMode |
|---|---|---|---|
| esp32 | 未填 | 已填（必填） | `auth-only` |
| esp32 | 已填 | 已填（必填） | `flash-then-auth` |
| other | 任意 | 已填（必填） | `auth-only`（固件被忽略，`canFlash=false`） |

**`flash-only` 模式已删除。**

---

## 变更范围

### 1. `src/features/batch-flash-auth/types.ts`

```ts
// 删除
export const BATCH_AUTH_SUPPORTED_CHIPS = ["esp32"] as const;

// 新增：本工具可用芯片（授权协议均支持）
// GD32 支持落地时追加 "gd32"
export const BATCH_AUTH_TOOL_CHIP_OPTIONS = ["esp32", "other"] as const;

// 新增：可烧录芯片子集（有注册的 flash plugin）
// GD32 支持落地时追加 "gd32"
export const BATCH_FLASH_CAPABLE_CHIPS = ["esp32"] as const;

// 修改：删除 "flash-only"
export type BatchOpMode = "auth-only" | "flash-then-auth";
```

---

### 2. `src/stores/batch-flash-auth.ts`

**默认 chipId**（`chipId` 当前不持久化，改默认值即可，无需迁移代码）：
```ts
// 之前
const chipId = ref<string>(DEFAULT_CHIP_ID);  // "t5ai"，不在新芯片列表中

// 之后
const chipId = ref<string>("esp32");
```

**删除** `authSupported` computed。

**新增** `canFlash` computed：
```ts
const canFlash = computed(() =>
  (BATCH_FLASH_CAPABLE_CHIPS as readonly string[]).includes(chipId.value)
);
```

**修改** `opMode`（移除 `authSupported` 依赖）：
```ts
const opMode = computed<BatchOpMode>(() =>
  canFlash.value && !!firmwarePath.value ? "flash-then-auth" : "auth-only"
);
```

**修改** `inputsValid`（Excel 必填，固件永不必填）：
```ts
const inputsValid = computed(() => !!authConfig.value.excelPath);
```

**修改** `showAuthStats`（授权统计始终显示）：
```ts
const showAuthStats = computed(() => true);
// 保留在 return 中，避免 BatchFlashAuthDashboard.vue 中的 v-if 引用崩溃
```

**修改** `startBatch`（删除 `flash-only` 分支）：
```ts
async function startBatch() {
  await startAuth(); // startAuth 内部已处理 flash-then-auth vs auth-only
}
```

**修改** `cancelPort` / `cancelAll`（删除 `flash-only` 三元判断，始终调用 auth cancel 命令）：
```ts
// 之前：opMode === "flash-only" ? "batch_flash_cancel_port" : "batch_auth_cancel_port"
// 之后：flash-then-auth 由 batch_auth_start 统一管理，cancel 共用同一 flag
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

**修改** store `return` 块：
- 删除 `authSupported`
- 新增 `canFlash`

---

### 3. `src/features/batch-flash-auth/BatchFlashAuthPage.vue`

```html
<!-- 之前 -->
<BatchAuthConfig v-if="store.authSupported" />

<!-- 之后：授权配置始终显示 -->
<BatchAuthConfig />
```

---

### 4. `src/features/batch-flash-auth/components/BatchFlashAuthConfig.vue`

**芯片选项来源**：从 `CHIP_IDS`（全部10种）改为 `BATCH_AUTH_TOOL_CHIP_OPTIONS`（`["esp32", "other"]`）。
i18n key 复用现有 `flash.chips.esp32` / `flash.chips.other`（"其他"已存在，无需新增）。

**固件区域**：整块 `v-if="store.canFlash"`（选 other 时直接不渲染）。
```html
<!-- 之前 -->
<div class="flex min-w-[16rem] flex-1 flex-col gap-1">
  <label>
    固件文件
    <span v-if="store.opMode !== 'auth-only'" class="text-[var(--ty-danger)]">*</span>
  </label>
  ...
</div>

<!-- 之后：整个固件区域条件渲染，移除 * 标记 -->
<div v-if="store.canFlash" class="flex min-w-[16rem] flex-1 flex-col gap-1">
  <label>固件文件</label>  <!-- 无 * -->
  ...
</div>
```

---

### 5. `src/features/batch-flash-auth/components/BatchFlashAuthToolbar.vue`

**Line 17**（确认对话框中 excelInfo）：
```ts
// 之前：guard flash-only
store.opMode !== "flash-only" && store.authConfig.excelPath
  ? `\n授权表：${store.authConfig.excelPath}` : ""

// 之后：opMode 永不为 flash-only，直接用 excelPath
store.authConfig.excelPath
  ? `\n授权表：${store.authConfig.excelPath}` : ""
```

**Line 21**（确认对话框中 firmwareInfo）：
```ts
// 之前
store.opMode !== "auth-only" && store.firmwarePath

// 之后（语义等价，但明确）
store.opMode === "flash-then-auth" && store.firmwarePath
```

**Line 25**（确认对话框标题）：无需改动，`auth-only` vs 非 `auth-only` 逻辑仍正确。

**Line 100**（start 按钮 tooltip，死代码清除）：
```html
<!-- 之前：固件未填时的提示（固件必填时代码） -->
:title="
  !store.firmwarePath && store.opMode !== 'auth-only'
    ? '请先选择固件文件'
    : !store.slots.some(...)
      ? '暂无空闲串口'
      : undefined
"

<!-- 之后：固件永不必填，移除第一个条件 -->
:title="
  !store.authConfig.excelPath
    ? '请先选择授权表'
    : !store.slots.some((s) => s.status === 'idle')
      ? '暂无空闲串口'
      : undefined
"
```

---

### 6. `src/features/batch-flash-auth/components/BatchFlashAuthDashboard.vue`

**Line 117**：`showAuthStats` 现在恒为 `true`，`v-if` 为冗余但无害。保留不改（spec 不要求改模板）。

---

### 7. `src/stores/batch-flash-auth.test.ts`

**删除以下测试块**：

| 位置 | 内容 | 原因 |
|---|---|---|
| `describe("opMode")` 内 lines 13–29 | 三条 `flash-only` 测试 | 模式已删除 |
| `describe("authSupported")` lines 47–67 | 整个块 | computed 已删除 |
| line 171–175 | `"canStart is false when firmware missing (flash-only mode)"` | 描述过时，逻辑已变 |

**修改以下测试**：

line 177–181 `"canStart is true when idle slot exists and inputs valid"`：
```ts
// 之前：只设 firmwarePath，无 Excel——新逻辑下 inputsValid=false，测试会失败
store.firmwarePath = "/fw.bin";

// 之后：必须同时设 Excel
store.chipId = "esp32";
store.firmwarePath = "/fw.bin";
store.authConfig.excelPath = "/auth.xlsx";
```

**新增测试**（`describe("opMode")` 内）：

```ts
it("is auth-only when chip=other regardless of firmware", () => {
  store.chipId = "other";
  store.firmwarePath = "/fw.bin";
  store.authConfig.excelPath = "/auth.xlsx";
  expect(store.opMode).toBe("auth-only");
});
```

**新增** `describe("canFlash")` 块：
```ts
describe("canFlash", () => {
  it("is true for esp32", () => {
    store.chipId = "esp32";
    expect(store.canFlash).toBe(true);
  });
  it("is false for other", () => {
    store.chipId = "other";
    expect(store.canFlash).toBe(false);
  });
});
```

**新增** `describe("inputsValid")` 块（补充）：
```ts
it("is false when no excel (esp32 with firmware)", () => {
  store.chipId = "esp32";
  store.firmwarePath = "/fw.bin";
  expect(store.inputsValid).toBe(false);
});
it("is true when excel set, no firmware (auth-only)", () => {
  store.chipId = "esp32";
  store.authConfig.excelPath = "/auth.xlsx";
  expect(store.inputsValid).toBe(true);
});
it("is true when chip=other with excel", () => {
  store.chipId = "other";
  store.authConfig.excelPath = "/auth.xlsx";
  expect(store.inputsValid).toBe(true);
});
```

---

## 扩展点

后续 GD32 支持落地时，**只需修改两处**：
1. `BATCH_AUTH_TOOL_CHIP_OPTIONS` 追加 `"gd32"`
2. `BATCH_FLASH_CAPABLE_CHIPS` 追加 `"gd32"`

其他逻辑（store、组件、测试）无需改动。

---

## 文件清单

| 文件 | 变更类型 |
|---|---|
| `src/features/batch-flash-auth/types.ts` | 常量 + 类型修改 |
| `src/stores/batch-flash-auth.ts` | 逻辑修改（chipId 默认值、4 个 computed、startBatch、return） |
| `src/features/batch-flash-auth/BatchFlashAuthPage.vue` | 删除 `v-if` |
| `src/features/batch-flash-auth/components/BatchFlashAuthConfig.vue` | 芯片列表 + 固件区条件渲染 + 移除 * |
| `src/features/batch-flash-auth/components/BatchFlashAuthToolbar.vue` | 清理 3 处 `opMode` 引用 |
| `src/stores/batch-flash-auth.test.ts` | 删除/修改/新增测试 |
