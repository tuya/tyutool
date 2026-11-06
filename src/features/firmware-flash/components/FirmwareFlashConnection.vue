<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useFirmwareFlashContext } from "../context";
import TySelect, { type TySelectOption } from "@/components/TySelect.vue";
import TyConnectionBar from "@/components/TyConnectionBar.vue";
import TyBaudRateInput from "@/components/TyBaudRateInput.vue";

const { t } = useI18n();
const ctx = useFirmwareFlashContext();

/** True when the authorize tab is active — baud rate then controls auth baud. */
const isAuthTab = computed(() => ctx.activeTab === "authorize");

const authBaudSelectVal = computed({
  get: () => String(ctx.selectedAuthBaudRate),
  set(val: string) {
    ctx.selectedAuthBaudRate = Number(val);
  },
});

const serialPortOptions = computed<TySelectOption[]>(() => {
  if (ctx.SERIAL_PORT_OPTIONS.length === 0) {
    return [
      { value: "", label: t("flash.noPortsPlaceholder"), disabled: true },
    ];
  }
  return ctx.SERIAL_PORT_OPTIONS.map((p) => ({
    value: p.value,
    label: p.label,
    ...(p.optionTooltip ? { optionTooltip: p.optionTooltip } : {}),
  }));
});

const standardBaudOptions = computed<TySelectOption[]>(() =>
  (ctx.BAUD_RATE_OPTIONS as readonly number[]).map((b) => ({
    value: String(b),
    label: String(b),
  })),
);

const chipOptions = computed<TySelectOption[]>(() =>
  (isAuthTab.value ? ctx.AUTH_CHIP_IDS : ctx.CHIP_IDS).map((c) => ({
    value: c,
    label: t(`flash.chips.${c}`),
  })),
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
</script>

<template>
  <TyConnectionBar :aria-label="t('flash.connectionAria')">
    <template #icon>
      <div
        class="conn-icon-wrap flex size-10 shrink-0 items-center justify-center rounded-xl"
        aria-hidden="true"
      >
        <FontAwesomeIcon :icon="['fas', 'plug']" class="size-[1.1rem]" />
      </div>
    </template>
    <template #status>
      <p class="conn-section-label">{{ t("flash.serialSection") }}</p>
      <div class="mt-0.5 flex items-center gap-2">
        <span
          class="conn-status-dot inline-block size-2 shrink-0 rounded-full"
          :class="ctx.connected ? 'conn-status-on' : 'conn-status-off'"
          aria-hidden="true"
        />
        <span class="conn-status-text text-xs font-semibold">{{
          ctx.statusText
        }}</span>
      </div>
    </template>
    <template #fields>
      <!-- 串口 -->
      <div class="flex min-w-0 items-center gap-1.5">
        <label
          for="serial-port"
          class="conn-field-label shrink-0 text-xs font-semibold"
        >
          {{ t("flash.serial") }}
        </label>
        <TySelect
          id="serial-port"
          v-model="serialPortValue"
          :options="serialPortOptions"
          :disabled="ctx.busy"
          :aria-describedby="
            ctx.portsRefreshing || ctx.portsRefreshError
              ? 'flash-port-feedback'
              : undefined
          "
          class="w-[13rem] min-w-0"
        />
      </div>

      <!-- 波特率（授权 Tab 时显示授权波特率，其余 Tab 显示烧录波特率） -->
      <div class="flex min-w-0 items-center gap-1.5">
        <label
          for="baud-rate"
          class="conn-field-label shrink-0 text-xs font-semibold"
        >
          {{ isAuthTab ? t("flash.authBaud") : t("flash.baud") }}
        </label>
        <TyBaudRateInput
          v-if="!isAuthTab"
          id="baud-rate"
          v-model="ctx.selectedBaudRate"
          :options="ctx.BAUD_RATE_OPTIONS"
          :disabled="ctx.busy"
          class="w-[8rem] min-w-0"
        />
        <TySelect
          v-else
          id="baud-rate"
          v-model="authBaudSelectVal"
          :options="standardBaudOptions"
          :disabled="ctx.busy"
          class="w-[8rem] min-w-0"
        />
      </div>

      <!-- 芯片 -->
      <div class="flex min-w-0 items-center gap-1.5">
        <label
          for="chip-select"
          class="conn-field-label shrink-0 text-xs font-semibold"
        >
          {{ t("flash.chip") }}
        </label>
        <TySelect
          id="chip-select"
          v-model="chipValue"
          :options="chipOptions"
          :disabled="ctx.busy"
          class="min-w-0 w-[9rem] max-w-[13rem]"
        />
      </div>
      <div
        v-if="ctx.portsRefreshing || ctx.portsRefreshError"
        id="flash-port-feedback"
        class="w-full flex flex-wrap items-center gap-2 text-xs text-[var(--ty-text-muted)]"
        :class="{
          'sr-only':
            ctx.portsRefreshing && ctx.portsScanned && !ctx.portsRefreshError,
        }"
      >
        <span v-if="ctx.portsRefreshing" role="status">{{
          t("common.loading")
        }}</span>
        <span v-else-if="ctx.portsRefreshError" role="alert">{{
          ctx.portsRefreshError
        }}</span>
      </div>
    </template>
  </TyConnectionBar>
</template>
