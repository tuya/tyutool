<script setup lang="ts">
import { computed, onActivated, onDeactivated, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { faCircleNotch } from "@fortawesome/free-solid-svg-icons";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { isTauriRuntime } from "@/runtime";
import {
  formatSerialPortLabel,
  type SerialPortDropdownOption,
  type TauriSerialPortRow,
} from "@/utils/serial-port-label";
import { wsTransport } from "@/transport/ws-transport";
import { useFlashStore } from "@/stores/flash";
import { rustPluginIdForChip } from "@/features/firmware-flash/chip-manifests";
import TySelect from "@/components/TySelect.vue";
import TyConnectionBar from "@/components/TyConnectionBar.vue";
import SerialDebugSettingsModal from "./SerialDebugSettingsModal.vue";

const s = useSerialDebugStore();
const flashStore = useFlashStore();
const { t } = useI18n();

const rawPortOptions = ref<SerialPortDropdownOption[]>([]);

const serialPortOptions = computed<SerialPortDropdownOption[]>(() => {
  if (rawPortOptions.value.length === 0) {
    return [
      { value: "", label: t("flash.noPortsPlaceholder"), disabled: true },
    ];
  }
  return rawPortOptions.value;
});
const useCustomBaud = ref(false);
const customBaudInput = ref("");
const showSettings = ref(false);

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
      const rows = await invoke<TauriSerialPortRow[]>("list_serial_ports_cmd");
      rawPortOptions.value = rows.map((p) => ({
        value: p.path,
        label: formatSerialPortLabel(p, t),
      }));
    } else {
      const rows = await wsTransport.listPorts();
      rawPortOptions.value = rows.map((p) => ({
        value: p.path,
        label: formatSerialPortLabel(p, t),
      }));
    }
  } catch {
    rawPortOptions.value = [];
  }
  if (rawPortOptions.value.length > 0) {
    const exists = rawPortOptions.value.some((p) => p.value === s.port);
    if (!exists) {
      s.port = rawPortOptions.value[0].value;
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

onMounted(() => {
  void refreshPorts();
});

onActivated(() => {
  if (!s.open) void refreshPorts();
});

onDeactivated(() => {
  showSettings.value = false;
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
            rawPortOptions.length === 0
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

      <button
        type="button"
        class="conn-btn-action flex items-center gap-1.5 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150"
        :disabled="!s.port.trim()"
        :aria-label="t('serialDebug.conn.deviceReset')"
        :title="t('serialDebug.conn.deviceResetHint')"
        @click="
          s.deviceReset(
            rustPluginIdForChip(flashStore.selectedChipId),
            flashStore.selectedSerialPort,
          )
        "
      >
        <FontAwesomeIcon
          :icon="['fas', 'power-off']"
          class="size-3.5 shrink-0"
          aria-hidden="true"
        />
        {{ t("serialDebug.conn.deviceReset") }}
      </button>

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
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
}
</style>
