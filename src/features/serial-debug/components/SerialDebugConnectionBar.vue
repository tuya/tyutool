<script setup lang="ts">
import { computed, onActivated, onDeactivated, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { faCircleNotch } from "@fortawesome/free-solid-svg-icons";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { isTauriRuntime } from "@/runtime";
import {
  formatSerialPortLabel,
  type TauriSerialPortRow,
  tuyaDualSerialHoverTooltip,
} from "@/utils/serial-port-label";
import { wsTransport } from "@/transport/ws-transport";
import { useFlashStore } from "@/stores/flash";
import {
  AUTH_ONLY_CHIP_ID,
  CHIP_IDS,
  DEFAULT_CHIP_ID,
  normalizeChipId,
} from "@/features/firmware-flash/constants";
import TySelect from "@/components/TySelect.vue";
import TyConnectionBar from "@/components/TyConnectionBar.vue";
import SerialDebugSettingsModal from "./SerialDebugSettingsModal.vue";

const s = useSerialDebugStore();
const flashStore = useFlashStore();
const { t } = useI18n();

type RebootDialogMode = "change" | "configure-and-reset";

const availablePortRows = ref<TauriSerialPortRow[]>([]);

const allKnownPortRows = computed<TauriSerialPortRow[]>(() => {
  if (
    !s.port.trim() ||
    availablePortRows.value.some((row) => row.path === s.port.trim())
  ) {
    return availablePortRows.value;
  }
  return [...availablePortRows.value, { path: s.port.trim() }];
});

const serialPortOptions = computed(() => {
  if (allKnownPortRows.value.length === 0) {
    return [
      { value: "", label: t("flash.noPortsPlaceholder"), disabled: true },
    ];
  }
  return allKnownPortRows.value.map((row) => ({
    value: row.path,
    label: formatSerialPortLabel(row, t),
  }));
});
const useCustomBaud = ref(false);
const customBaudInput = ref("");
const showSettings = ref(false);
const showRebootTargetDialog = ref(false);
const rebootDialogMode = ref<RebootDialogMode>("change");
const draftRebootControlPort = ref("");
const draftRebootChipId = ref(DEFAULT_CHIP_ID);

const rebootResolution = computed(() =>
  s.resolveRebootTarget(allKnownPortRows.value.map((row) => row.path)),
);

const rebootControlPortLabel = computed(
  () =>
    rebootResolution.value.controlPort ??
    t("serialDebug.conn.rebootControlPortUnset"),
);

const rebootControlPortOptions = computed(() =>
  allKnownPortRows.value.map((row) => ({
    value: row.path,
    label: formatSerialPortLabel(row, t),
    optionTooltip:
      tuyaDualSerialHoverTooltip(row.usbVid, row.usbPid, row.usbInterface, t) ??
      undefined,
  })),
);

const rebootChipOptions = computed(() =>
  CHIP_IDS.map((chipId) => ({
    value: chipId,
    label: t(`flash.chips.${chipId}`),
  })),
);

const baudOptions = computed(() => {
  const opts = s.commonBaudRates.map((r) => ({
    value: String(r),
    label: String(r),
  }));
  opts.push({ value: "custom", label: t("serialDebug.conn.customBaud") });
  return opts;
});

const selectedBaudValue = computed({
  get: () => (useCustomBaud.value ? "custom" : String(s.baudRate)),
  set: (v: string) => {
    if (v === "custom") {
      useCustomBaud.value = true;
      customBaudInput.value = customBaudInput.value || String(s.baudRate);
      s.customBaudRate = parseInt(customBaudInput.value, 10) || s.baudRate;
    } else {
      useCustomBaud.value = false;
      s.customBaudRate = null;
      s.baudRate = parseInt(v, 10);
    }
  },
});

function onCustomBaudInput(v: string): void {
  customBaudInput.value = v;
  const n = parseInt(v, 10);
  s.customBaudRate = Number.isFinite(n) && n > 0 ? n : null;
}

const canOpen = computed(
  () => !s.opening && !!s.port.trim() && (s.customBaudRate ?? s.baudRate) > 0,
);

async function refreshPorts(): Promise<void> {
  try {
    if (isTauriRuntime()) {
      const { invoke } = await import("@tauri-apps/api/core");
      availablePortRows.value = await invoke<TauriSerialPortRow[]>(
        "list_serial_ports_cmd",
      );
    } else {
      availablePortRows.value = await wsTransport.listPorts();
    }
  } catch {
    availablePortRows.value = [];
  }
  if (allKnownPortRows.value.length > 0) {
    const exists = allKnownPortRows.value.some((row) => row.path === s.port);
    if (!exists) {
      s.port = allKnownPortRows.value[0].path;
    }
  }
}

async function toggleOpen(): Promise<void> {
  if (s.open) {
    await s.closePort();
  } else {
    await s.openPort();
  }
}

function normalizedSelectedChipId(): string | null {
  const normalized = normalizeChipId(flashStore.selectedChipId.trim());
  if (!normalized || normalized === AUTH_ONLY_CHIP_ID) {
    return null;
  }
  return normalized;
}

function isFlashAuthRole(role: string | null | undefined): boolean {
  return role?.trim() === "flash_auth";
}

function preferredRebootControlPort(): string {
  const rows = allKnownPortRows.value;
  if (rows.length === 0) {
    return s.port.trim();
  }
  const remembered = s.rebootControlPort.trim();
  if (remembered && rows.some((row) => row.path === remembered)) {
    return remembered;
  }
  const flashPort = flashStore.selectedSerialPort.trim();
  if (flashPort && rows.some((row) => row.path === flashPort)) {
    return flashPort;
  }
  const flashAuthPort = rows.find((row) => isFlashAuthRole(row.portRole))?.path;
  if (flashAuthPort) {
    return flashAuthPort;
  }
  const otherThanLogPort = rows.find((row) => row.path !== s.port.trim())?.path;
  if (otherThanLogPort) {
    return otherThanLogPort;
  }
  return rows[0].path;
}

function openRebootTargetDialog(mode: RebootDialogMode): void {
  rebootDialogMode.value = mode;
  draftRebootControlPort.value =
    rebootResolution.value.controlPort ?? preferredRebootControlPort();
  draftRebootChipId.value =
    rebootResolution.value.chipId ??
    normalizedSelectedChipId() ??
    DEFAULT_CHIP_ID;
  showRebootTargetDialog.value = true;
}

async function confirmRebootTarget(): Promise<void> {
  if (!draftRebootControlPort.value || !draftRebootChipId.value) {
    return;
  }
  s.rememberRebootTarget(draftRebootControlPort.value, draftRebootChipId.value);
  showRebootTargetDialog.value = false;
  if (rebootDialogMode.value === "configure-and-reset") {
    await s.deviceReset(draftRebootChipId.value, draftRebootControlPort.value);
  }
}

async function triggerDeviceReset(): Promise<void> {
  if (
    rebootResolution.value.needsSelection ||
    !rebootResolution.value.controlPort ||
    !rebootResolution.value.chipId
  ) {
    openRebootTargetDialog("configure-and-reset");
    return;
  }
  await s.deviceReset(
    rebootResolution.value.chipId,
    rebootResolution.value.controlPort,
  );
}

onMounted(() => {
  void refreshPorts();
});

onActivated(() => {
  if (!s.open) void refreshPorts();
});

onDeactivated(() => {
  showSettings.value = false;
  showRebootTargetDialog.value = false;
});
</script>

<template>
  <TyConnectionBar :aria-label="t('serialDebug.pageTitle')">
    <template #icon>
      <div
        class="conn-icon-wrap flex size-10 shrink-0 items-center justify-center rounded-xl"
        aria-hidden="true"
      >
        <FontAwesomeIcon :icon="['fas', 'plug']" class="size-[1.1rem]" />
      </div>
    </template>
    <template #status>
      <p class="conn-section-label">{{ t("serialDebug.conn.port") }}</p>
      <div class="mt-0.5 flex items-center gap-1.5">
        <span
          class="conn-status-dot inline-block size-2 shrink-0 rounded-full"
          :class="s.open ? 'conn-status-on' : 'conn-status-off'"
          aria-hidden="true"
        />
        <span class="conn-status-text text-xs font-semibold">
          {{
            s.opening
              ? t("serialDebug.conn.connecting")
              : s.open
                ? t("serialDebug.conn.statusConnected")
                : t("serialDebug.conn.statusDisconnected")
          }}
        </span>
      </div>
    </template>
    <template #fields>
      <div class="flex min-w-0 items-center gap-1.5">
        <label
          for="sd-port"
          class="conn-field-label shrink-0 text-xs font-semibold"
          >{{ t("serialDebug.conn.port") }}</label
        >
        <TySelect
          id="sd-port"
          v-model="s.port"
          :options="serialPortOptions"
          :placeholder="
            allKnownPortRows.length === 0
              ? t('flash.noPortsPlaceholder')
              : undefined
          "
          :disabled="s.open || s.opening"
          class="min-w-[12rem]"
          @open="refreshPorts"
        />
      </div>

      <div class="flex min-w-0 items-center gap-1.5">
        <label
          for="sd-baud"
          class="conn-field-label shrink-0 text-xs font-semibold"
          >{{ t("serialDebug.conn.baud") }}</label
        >
        <TySelect
          id="sd-baud"
          :model-value="selectedBaudValue"
          :options="baudOptions"
          :disabled="s.open"
          @update:model-value="selectedBaudValue = $event"
          class="w-[8rem]"
        />
        <input
          v-if="useCustomBaud"
          type="number"
          min="1"
          class="conn-select w-[6.5rem] min-w-0"
          :value="customBaudInput"
          :disabled="s.open"
          @input="
            (e) => onCustomBaudInput((e.target as HTMLInputElement).value)
          "
        />
      </div>
    </template>
    <template #actions>
      <button
        type="button"
        class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        :aria-label="t('serialDebug.conn.settings')"
        @click="showSettings = true"
      >
        <FontAwesomeIcon
          :icon="['fas', 'gear']"
          class="size-3.5 shrink-0"
          aria-hidden="true"
        />
        {{ t("serialDebug.conn.settings") }}
      </button>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
          :disabled="!s.port.trim() && allKnownPortRows.length === 0"
          :aria-label="t('serialDebug.conn.deviceReset')"
          :title="t('serialDebug.conn.deviceResetHint')"
          @click="triggerDeviceReset"
        >
          <FontAwesomeIcon
            :icon="['fas', 'power-off']"
            class="size-3.5 shrink-0"
            aria-hidden="true"
          />
          {{ t("serialDebug.conn.deviceReset") }}
        </button>
        <span class="text-xs text-[var(--ty-text-muted)]">
          {{ t("serialDebug.conn.rebootControlPort") }}:
          {{ rebootControlPortLabel }}
        </span>
        <button
          type="button"
          class="conn-btn-action rounded-lg px-2.5 py-2 text-xs font-semibold transition-all duration-150"
          :aria-label="t('serialDebug.conn.changeRebootTarget')"
          @click="openRebootTargetDialog('change')"
        >
          {{ t("serialDebug.conn.changeRebootTarget") }}
        </button>
      </div>

      <button
        v-if="!s.open"
        type="button"
        class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        :disabled="!canOpen"
        @click="toggleOpen"
      >
        <FontAwesomeIcon
          v-if="s.opening"
          :icon="faCircleNotch"
          class="fa-spin size-3.5 shrink-0"
          aria-hidden="true"
        />
        <FontAwesomeIcon
          v-else
          :icon="['fas', 'plug']"
          class="size-3.5 shrink-0"
          aria-hidden="true"
        />
        {{
          s.opening
            ? t("serialDebug.conn.connecting")
            : t("serialDebug.conn.open")
        }}
      </button>
      <button
        v-else
        type="button"
        class="conn-btn-disconnect flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        @click="toggleOpen"
      >
        <FontAwesomeIcon
          :icon="['fas', 'plug-circle-xmark']"
          class="size-3.5 shrink-0"
          aria-hidden="true"
        />
        {{ t("serialDebug.conn.close") }}
      </button>
    </template>
  </TyConnectionBar>

  <Teleport to="body">
    <SerialDebugSettingsModal
      v-if="showSettings"
      @close="showSettings = false"
    />
  </Teleport>
  <Teleport to="body">
    <div
      v-if="showRebootTargetDialog"
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      role="dialog"
      aria-modal="true"
      :aria-label="t('serialDebug.conn.rebootTargetDialogTitle')"
      @click.self="showRebootTargetDialog = false"
    >
      <div class="ty-card w-[min(90vw,420px)] overflow-hidden">
        <div
          class="flex items-center justify-between border-b border-[var(--ty-border)] px-4 py-3"
        >
          <h2 class="text-sm font-semibold text-[var(--ty-text)]">
            {{ t("serialDebug.conn.rebootTargetDialogTitle") }}
          </h2>
          <button
            type="button"
            class="page-header-btn flex size-8 items-center justify-center rounded-lg"
            :aria-label="t('common.closeDialog')"
            @click="showRebootTargetDialog = false"
          >
            <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-3.5" />
          </button>
        </div>
        <div class="flex flex-col gap-4 p-4">
          <label class="field">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{
              t("serialDebug.conn.rebootControlPort")
            }}</span>
            <TySelect
              v-model="draftRebootControlPort"
              :options="rebootControlPortOptions"
            />
          </label>

          <label class="field">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{
              t("serialDebug.conn.rebootTargetChip")
            }}</span>
            <TySelect
              v-model="draftRebootChipId"
              :options="rebootChipOptions"
            />
          </label>
        </div>
        <div
          class="flex items-center justify-end gap-2 border-t border-[var(--ty-border)] px-4 py-3"
        >
          <button
            type="button"
            class="conn-btn-action rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
            @click="showRebootTargetDialog = false"
          >
            {{ t("common.closeDialog") }}
          </button>
          <button
            type="button"
            class="conn-btn-action rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
            :disabled="!draftRebootControlPort || !draftRebootChipId"
            @click="confirmRebootTarget"
          >
            {{ t("serialDebug.conn.rebootTargetConfirm") }}
          </button>
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
</style>
