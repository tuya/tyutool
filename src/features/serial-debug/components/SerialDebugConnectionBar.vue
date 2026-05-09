<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useSerialDebugStore } from '@/stores/serial-debug';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';
import { formatSerialPortLabel, type SerialPortDropdownOption, type TauriSerialPortRow } from '@/features/firmware-flash/serial-port-label';
import { wsTransport } from '@/features/firmware-flash/ws-transport';
import TySelect from '@/components/TySelect.vue';

const s = useSerialDebugStore();
const { t } = useI18n();

const serialPortOptions = ref<SerialPortDropdownOption[]>([]);
const useCustomBaud = ref(false);
const customBaudInput = ref('');

const dataBitsOptions = computed(() => [
  { value: 'five', label: '5' },
  { value: 'six', label: '6' },
  { value: 'seven', label: '7' },
  { value: 'eight', label: '8' },
]);
const parityOptions = computed(() => [
  { value: 'none', label: t('serialDebug.conn.parityNone') },
  { value: 'odd', label: t('serialDebug.conn.parityOdd') },
  { value: 'even', label: t('serialDebug.conn.parityEven') },
]);
const stopBitsOptions = computed(() => [
  { value: 'one', label: '1' },
  { value: 'onePointFive', label: '1.5' },
  { value: 'two', label: '2' },
]);
const baudOptions = computed(() => {
  const opts = s.commonBaudRates.map((r) => ({ value: String(r), label: String(r) }));
  opts.push({ value: 'custom', label: t('serialDebug.conn.customBaud') });
  return opts;
});

const selectedBaudValue = computed({
  get: () => (useCustomBaud.value ? 'custom' : String(s.baudRate)),
  set: (v: string) => {
    if (v === 'custom') {
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

const canOpen = computed(() => !s.opening && !!s.port.trim() && (s.customBaudRate ?? s.baudRate) > 0);

async function refreshPorts(): Promise<void> {
  if (isTauriRuntime()) {
    const { invoke } = await import('@tauri-apps/api/core');
    const rows = await invoke<TauriSerialPortRow[]>('list_serial_ports_cmd');
    serialPortOptions.value = rows.map((p) => ({ value: p.path, label: formatSerialPortLabel(p, t) }));
  } else {
    try {
      const rows = await wsTransport.listPorts();
      serialPortOptions.value = rows.map((p) => ({ value: p.path, label: formatSerialPortLabel(p, t) }));
    } catch {
      serialPortOptions.value = [];
    }
  }
  if (!s.port && serialPortOptions.value.length > 0) {
    s.port = serialPortOptions.value[0].value;
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
</script>

<template>
  <div class="conn-bar flex flex-wrap items-end gap-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-3">
    <label class="field">
      <span class="label">{{ t('serialDebug.conn.port') }}</span>
      <TySelect v-model="s.port" :options="serialPortOptions" :disabled="s.open || s.opening" class="min-w-[14rem]" />
    </label>

    <button type="button" class="btn-icon" :disabled="s.open || s.opening" :aria-label="t('serialDebug.conn.refresh')" @click="refreshPorts">
      <FontAwesomeIcon :icon="['fas', 'rotate']" />
    </button>

    <label class="field">
      <span class="label">{{ t('serialDebug.conn.baud') }}</span>
      <TySelect :model-value="selectedBaudValue" :options="baudOptions" :disabled="s.open" @update:model-value="selectedBaudValue = $event" />
    </label>

    <label v-if="useCustomBaud" class="field">
      <span class="label">{{ t('serialDebug.conn.customBaud') }}</span>
      <input type="number" min="1" class="input" :value="customBaudInput" :disabled="s.open" @input="(e) => onCustomBaudInput((e.target as HTMLInputElement).value)" />
    </label>

    <label class="field">
      <span class="label">{{ t('serialDebug.conn.dataBits') }}</span>
      <TySelect v-model="s.dataBits" :options="dataBitsOptions" :disabled="s.open" />
    </label>

    <label class="field">
      <span class="label">{{ t('serialDebug.conn.parity') }}</span>
      <TySelect v-model="s.parity" :options="parityOptions" :disabled="s.open" />
    </label>

    <label class="field">
      <span class="label">{{ t('serialDebug.conn.stopBits') }}</span>
      <TySelect v-model="s.stopBits" :options="stopBitsOptions" :disabled="s.open" />
    </label>

    <label class="toggle flex items-center gap-1 text-xs">
      <input type="checkbox" v-model="s.autoRelease" />
      <span>{{ t('serialDebug.conn.autoRelease') }}</span>
      <span class="tooltip" :title="t('serialDebug.conn.autoReleaseTip')">ⓘ</span>
    </label>

    <label class="toggle flex items-center gap-1 text-xs">
      <input type="checkbox" v-model="s.hexView" />
      <span>{{ t('serialDebug.conn.hexView') }}</span>
    </label>

    <div class="ml-auto flex gap-2">
      <button type="button" class="btn-secondary" @click="s.clear()">{{ t('serialDebug.conn.clear') }}</button>
      <button type="button" class="btn-primary" :disabled="!canOpen && !s.open" @click="toggleOpen">
        {{ s.open ? t('serialDebug.conn.close') : t('serialDebug.conn.open') }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.field { display: flex; flex-direction: column; gap: 0.25rem; }
.label { font-size: 0.7rem; color: var(--ty-text-muted); }
.input { border: 1px solid var(--ty-border); background: var(--ty-canvas); border-radius: 0.5rem; padding: 0.375rem 0.5rem; font-size: 0.875rem; min-width: 7rem; }
.btn-icon { padding: 0.5rem 0.625rem; border: 1px solid var(--ty-border); border-radius: 0.5rem; background: var(--ty-canvas); }
.btn-primary { padding: 0.5rem 1rem; background: var(--ty-primary); color: white; border-radius: 0.5rem; font-weight: 600; }
.btn-primary:disabled { opacity: 0.5; cursor: not-allowed; }
.btn-secondary { padding: 0.5rem 1rem; border: 1px solid var(--ty-border); border-radius: 0.5rem; }
.tooltip { color: var(--ty-text-muted); cursor: help; }
</style>
