<script setup lang="ts">
import { ref, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { useFirmwareFlashContext } from '../context';
import TySelect, { type TySelectOption } from '@/components/TySelect.vue';

const { t } = useI18n();
const ctx = useFirmwareFlashContext();

// Only destructure actions (functions) — ref state must be accessed via ctx.xxx
// to preserve Pinia reactive() wrapper (destructuring unwraps refs into snapshots).
const { refreshDevice, connect, disconnect, deviceReset } = ctx;

/** True when the authorize tab is active — baud rate then controls auth baud. */
const isAuthTab = computed(() => ctx.activeTab === 'authorize');

const baudIsCustom = ref(!(ctx.BAUD_RATE_OPTIONS as readonly number[]).includes(ctx.selectedBaudRate));

const baudSelectVal = computed({
  get() {
    if (isAuthTab.value) {
      return String(ctx.selectedAuthBaudRate);
    }
    return baudIsCustom.value ? 'custom' : String(ctx.selectedBaudRate);
  },
  set(val: string) {
    if (isAuthTab.value) {
      // Auth baud only has standard options — no custom entry.
      ctx.selectedAuthBaudRate = Number(val);
      return;
    }
    if (val === 'custom') {
      baudIsCustom.value = true;
    } else {
      baudIsCustom.value = false;
      ctx.selectedBaudRate = Number(val);
    }
  },
});

const serialPortOptions = computed<TySelectOption[]>(() => {
  if (ctx.SERIAL_PORT_OPTIONS.length === 0) {
    return [{ value: '', label: t('flash.noPortsPlaceholder'), disabled: true }];
  }
  return ctx.SERIAL_PORT_OPTIONS.map(p => ({
    value: p.value,
    label: p.label,
    ...(p.optionTooltip ? { optionTooltip: p.optionTooltip } : {}),
  }));
});

const baudOptions = computed<TySelectOption[]>(() => [
  { value: 'custom', label: t('flash.baudCustom') },
  ...(ctx.BAUD_RATE_OPTIONS as readonly number[]).map(b => ({ value: String(b), label: String(b) })),
]);

/** For authorize tab: standard options only, no custom entry. */
const baudOptionsNoCustom = computed<TySelectOption[]>(() =>
  (ctx.BAUD_RATE_OPTIONS as readonly number[]).map(b => ({ value: String(b), label: String(b) }))
);

const chipOptions = computed<TySelectOption[]>(() =>
  (ctx.CHIP_IDS as readonly string[]).map(c => ({ value: c, label: t(`flash.chips.${c}`) }))
);

const serialPortValue = computed({
  get: () => ctx.selectedSerialPort,
  set: (val: string) => {
    ctx.selectedSerialPort = val;
  },
});

const chipValue = computed({
  get: () => ctx.selectedChipId,
  set: (val: string) => {
    ctx.selectedChipId = val;
  },
});

const deviceResetHintTitle = computed(() => t(`flash.deviceResetHints.${ctx.selectedChipId}`));
</script>

<template>
  <section
    class="conn-bar relative flex min-w-0 flex-col gap-3 overflow-hidden rounded-2xl p-3 sm:flex-row sm:items-center sm:gap-4 sm:p-3.5"
    :aria-label="t('flash.connectionAria')"
  >
    <!-- 背景装饰 -->
    <div class="conn-bar-bg pointer-events-none absolute inset-0" aria-hidden="true" />

    <!-- 状态指示器 -->
    <div class="relative flex shrink-0 items-center gap-3">
      <div class="conn-icon-wrap flex size-10 shrink-0 items-center justify-center rounded-xl" aria-hidden="true">
        <FontAwesomeIcon :icon="['fas', 'plug']" class="size-[1.1rem]" />
      </div>
      <div class="shrink-0">
        <p class="conn-section-label">{{ t('flash.serialSection') }}</p>
        <div class="mt-0.5 flex items-center gap-2">
          <span
            class="conn-status-dot inline-block size-2 shrink-0 rounded-full"
            :class="ctx.connected ? 'conn-status-on' : 'conn-status-off'"
            aria-hidden="true"
          />
          <span class="conn-status-text text-xs font-semibold">{{ ctx.statusText }}</span>
        </div>
      </div>
      <!-- 竖分隔线 -->
      <div class="conn-divider ml-1 hidden h-8 w-px shrink-0 sm:block" aria-hidden="true" />
    </div>

    <!-- 参数选择区 -->
    <div class="relative flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-2">
      <!-- 串口 -->
      <div class="flex min-w-0 items-center gap-1.5">
        <label for="serial-port" class="conn-field-label shrink-0 text-xs font-semibold">
          {{ t('flash.serial') }}
        </label>
        <TySelect
          id="serial-port"
          v-model="serialPortValue"
          :options="serialPortOptions"
          :disabled="ctx.busy || ctx.connected"
          class="w-auto min-w-[8.5rem] max-w-[16rem]"
          @open="refreshDevice"
        />
      </div>

      <!-- 波特率（授权 Tab 时显示授权波特率，其余 Tab 显示烧录波特率） -->
      <div class="flex min-w-0 items-center gap-1.5">
        <label for="baud-rate" class="conn-field-label shrink-0 text-xs font-semibold">
          {{ isAuthTab ? t('flash.authBaud') : t('flash.baud') }}
        </label>
        <TySelect
          id="baud-rate"
          v-model="baudSelectVal"
          :options="isAuthTab ? baudOptionsNoCustom : baudOptions"
          :disabled="ctx.busy || ctx.connected"
          class="w-[8rem] min-w-0"
        />
        <input
          v-if="baudIsCustom && !isAuthTab"
          id="baud-rate-custom"
          v-model.number="ctx.selectedBaudRate"
          type="number"
          min="300"
          max="4000000"
          step="1"
          class="conn-select w-[6.5rem] min-w-0 text-sm"
          :disabled="ctx.busy || ctx.connected"
        />
      </div>

      <!-- 芯片 -->
      <div class="flex min-w-0 items-center gap-1.5">
        <label for="chip-select" class="conn-field-label shrink-0 text-xs font-semibold">
          {{ t('flash.chip') }}
        </label>
        <TySelect
          id="chip-select"
          v-model="chipValue"
          :options="chipOptions"
          :disabled="ctx.busy || ctx.connected"
          class="min-w-0 w-[9rem] max-w-[13rem]"
        />
      </div>
    </div>

    <!-- 操作按钮 -->
    <div class="relative flex shrink-0 items-center gap-2">
      <button
        type="button"
        class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        :disabled="ctx.connected || ctx.busy"
        @click="refreshDevice"
      >
        <FontAwesomeIcon :icon="['fas', 'arrows-rotate']" class="size-3.5 shrink-0" aria-hidden="true" />
        {{ t('flash.refresh') }}
      </button>
      <button
        type="button"
        class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        :disabled="!ctx.selectedSerialPort || ctx.busy"
        :aria-label="t('flash.deviceReset')"
        :title="deviceResetHintTitle"
        @click="deviceReset"
      >
        <FontAwesomeIcon :icon="['fas', 'power-off']" class="size-3.5 shrink-0" aria-hidden="true" />
        {{ t('flash.deviceReset') }}
      </button>
      <button
        v-if="!ctx.connected"
        type="button"
        class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        :disabled="!ctx.selectedSerialPort || ctx.busy"
        @click="connect"
      >
        <FontAwesomeIcon :icon="['fas', 'plug']" class="size-3.5 shrink-0" aria-hidden="true" />
        {{ t('flash.connect') }}
      </button>
      <button
        v-else
        type="button"
        class="conn-btn-disconnect flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        @click="disconnect"
      >
        <FontAwesomeIcon :icon="['fas', 'plug-circle-xmark']" class="size-3.5 shrink-0" aria-hidden="true" />
        {{ t('flash.disconnect') }}
      </button>
    </div>
  </section>
</template>
