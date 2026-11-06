<script setup lang="ts">
import {
  computed,
  onActivated,
  onDeactivated,
  onMounted,
  onBeforeUnmount,
  ref,
  watch,
} from "vue";
import { useModalFocus } from "@/composables/useModalFocus";
import { useI18n } from "vue-i18n";
import { faCircleNotch } from "@fortawesome/free-solid-svg-icons";
import { useSerialDebugStore } from "@/stores/serial-debug";
import {
  formatSerialPortLabel,
  type TauriSerialPortRow,
  tuyaDualSerialHoverTooltip,
} from "@/utils/serial-port-label";
import { serialPortDiscovery } from "@/features/serial-port-indicators/discovery";
import { useFlashStore } from "@/stores/flash";
import {
  AUTH_ONLY_CHIP_ID,
  CHIP_IDS,
  DEFAULT_CHIP_ID,
  normalizeChipId,
  type ChipId,
} from "@/features/firmware-flash/constants";
import TySelect from "@/components/TySelect.vue";
import TyBaudRateInput from "@/components/TyBaudRateInput.vue";
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
    label: availablePortRows.value.some((p) => p.path === row.path)
      ? formatSerialPortLabel(row, t)
      : `${row.path} — ${t("serialDebug.conn.portUnavailable")}`,
    disabled: !availablePortRows.value.some((p) => p.path === row.path),
    optionTooltip:
      tuyaDualSerialHoverTooltip(row.usbVid, row.usbPid, row.usbInterface, t) ??
      undefined,
  }));
});
const portsLoading = ref(false);
const portsError = ref("");
let portsGeneration = 0;
const selectedPortAvailable = computed(() =>
  availablePortRows.value.some((row) => row.path === s.port.trim()),
);
const showSettings = ref(false);
const showRebootTargetDialog = ref(false);
const rebootDialogRef = ref<HTMLElement | null>(null);
useModalFocus(
  rebootDialogRef,
  () => showRebootTargetDialog.value,
  () => {
    showRebootTargetDialog.value = false;
  },
);
const rebootDialogMode = ref<RebootDialogMode>("change");
const draftRebootControlPort = ref("");
const draftRebootChipId = ref<ChipId>(DEFAULT_CHIP_ID);

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

const baudRateValid = ref(true);
const selectedBaudRate = computed({
  get: () => s.customBaudRate ?? s.baudRate,
  set: (value: number) => {
    if (s.commonBaudRates.some((rate) => rate === value)) {
      s.customBaudRate = null;
      s.baudRate = value;
    } else {
      s.customBaudRate = value;
    }
  },
});

const portFeedback = computed(() =>
  portsError.value
    ? t("serialDebug.conn.portsFailed", { msg: portsError.value })
    : s.port && !selectedPortAvailable.value
      ? t("serialDebug.conn.rememberedUnavailable", { port: s.port })
      : "",
);
const canOpen = computed(
  () =>
    !s.opening &&
    !portsLoading.value &&
    !portsError.value &&
    selectedPortAvailable.value &&
    baudRateValid.value &&
    selectedBaudRate.value > 0,
);

function selectAvailablePort(): void {
  if (s.open || s.opening || selectedPortAvailable.value) return;
  const preferred =
    availablePortRows.value.find((row) => row.portRole?.trim() === "log") ??
    availablePortRows.value[0];
  if (preferred) s.port = preferred.path;
}

watch(
  () => s.open || s.opening,
  (busy) => {
    if (!busy && !portsLoading.value && !portsError.value)
      selectAvailablePort();
  },
);

async function refreshPorts(): Promise<void> {
  if (portsLoading.value) return;
  const generation = ++portsGeneration;
  portsLoading.value = true;
  portsError.value = "";
  try {
    const rows = await serialPortDiscovery.listPorts();
    if (generation !== portsGeneration) return;
    availablePortRows.value = rows;
    selectAvailablePort();
  } catch (error) {
    if (generation === portsGeneration)
      portsError.value = error instanceof Error ? error.message : String(error);
  } finally {
    if (generation === portsGeneration) portsLoading.value = false;
  }
}
onBeforeUnmount(() => {
  portsGeneration++;
});

async function toggleOpen(): Promise<void> {
  if (s.open) {
    await s.closePort();
  } else if (canOpen.value) {
    await s.openPort();
  }
}

function isChipId(chipId: string): chipId is ChipId {
  return (CHIP_IDS as readonly string[]).includes(chipId);
}

function normalizedSelectedChipId(): ChipId | null {
  const normalized = normalizeChipId(flashStore.selectedChipId.trim());
  if (
    !normalized ||
    normalized === AUTH_ONLY_CHIP_ID ||
    !isChipId(normalized)
  ) {
    return null;
  }
  return normalized;
}

function normalizedRebootChipId(
  chipId: string | null | undefined,
): ChipId | null {
  const normalized = chipId?.trim() ? normalizeChipId(chipId.trim()) : "";
  return normalized && isChipId(normalized) ? normalized : null;
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
    normalizedRebootChipId(rebootResolution.value.chipId) ??
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

const stopDeviceNotifications = serialPortDiscovery.subscribe(() => {
  void refreshPorts();
});
onBeforeUnmount(stopDeviceNotifications);

onMounted(() => {
  void refreshPorts();
});

let firstActivation = true;
onActivated(() => {
  // KeepAlive also activates on initial mount, which already scans above.
  if (firstActivation) {
    firstActivation = false;
    return;
  }
  if (!s.open) void refreshPorts();
});

onDeactivated(() => {
  // A scan started for the previous activation must not become the new
  // activation's result (or error) through the shared pending request.
  if (portsLoading.value) serialPortDiscovery.invalidate();
  portsGeneration++;
  portsLoading.value = false;
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
      <h1 class="text-sm font-semibold text-[var(--ty-text)]">
        {{ t("serialDebug.pageTitle") }}
      </h1>
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
        <TySelect
          id="sd-port"
          v-model="s.port"
          :options="serialPortOptions"
          :aria-label="t('serialDebug.conn.port')"
          :aria-describedby="portFeedback ? 'sd-port-feedback' : undefined"
          :placeholder="
            allKnownPortRows.length === 0
              ? t('flash.noPortsPlaceholder')
              : undefined
          "
          :disabled="s.open || s.opening"
          class="w-[13rem] min-w-0"
        />
      </div>

      <div class="flex min-w-0 items-center gap-1.5">
        <label
          for="sd-baud"
          class="conn-field-label shrink-0 text-xs font-semibold"
          >{{ t("serialDebug.conn.baud") }}</label
        >
        <TyBaudRateInput
          id="sd-baud"
          v-model="selectedBaudRate"
          :options="s.commonBaudRates"
          :disabled="s.open || s.opening"
          @validity-change="baudRateValid = $event"
          class="w-[8rem]"
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
  <p
    v-if="portFeedback"
    id="sd-port-feedback"
    :role="portsError ? 'alert' : 'status'"
    class="text-xs text-[var(--ty-danger)]"
  >
    {{ portFeedback }}
  </p>

  <Teleport to="body">
    <SerialDebugSettingsModal
      v-if="showSettings"
      @close="showSettings = false"
    />
  </Teleport>
  <Teleport to="body">
    <div
      v-if="showRebootTargetDialog"
      class="fixed inset-0 z-50 flex items-center justify-center bg-[var(--ty-scrim)]"
      @click.self="showRebootTargetDialog = false"
    >
      <div
        ref="rebootDialogRef"
        tabindex="-1"
        role="dialog"
        aria-modal="true"
        :aria-label="t('serialDebug.conn.rebootTargetDialogTitle')"
        class="ty-card w-[min(90vw,420px)] max-h-[calc(100dvh-2rem)] overflow-y-auto shadow-[var(--ty-overlay-shadow)]"
      >
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
              :aria-label="t('serialDebug.conn.rebootControlPort')"
            />
          </label>

          <label class="field">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{
              t("serialDebug.conn.rebootTargetChip")
            }}</span>
            <TySelect
              v-model="draftRebootChipId"
              :options="rebootChipOptions"
              :aria-label="t('serialDebug.conn.rebootTargetChip')"
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
