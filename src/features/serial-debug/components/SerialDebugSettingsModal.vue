<script setup lang="ts">
import { computed, ref } from "vue";
import { useModalFocus } from "@/composables/useModalFocus";
import { useI18n } from "vue-i18n";
import { useSerialDebugStore } from "@/stores/serial-debug";
import TySelect from "@/components/TySelect.vue";
import TySwitch from "@/components/TySwitch.vue";

const emit = defineEmits<{ close: [] }>();
const s = useSerialDebugStore();
const { t } = useI18n();
const dialogRef = ref<HTMLElement | null>(null);
useModalFocus(dialogRef, () => true, close);

const dataBitsOptions = computed(() => [
  { value: "five", label: "5" },
  { value: "six", label: "6" },
  { value: "seven", label: "7" },
  { value: "eight", label: "8" },
]);
const parityOptions = computed(() => [
  { value: "none", label: t("serialDebug.conn.parityNone") },
  { value: "odd", label: t("serialDebug.conn.parityOdd") },
  { value: "even", label: t("serialDebug.conn.parityEven") },
]);
const stopBitsOptions = computed(() => [
  { value: "one", label: "1" },
  { value: "onePointFive", label: "1.5" },
  { value: "two", label: "2" },
]);

function close(): void {
  emit("close");
}
</script>

<template>
  <div
    ref="dialogRef"
    tabindex="-1"
    class="fixed inset-0 z-50 flex items-center justify-center bg-[var(--ty-scrim)]"
    role="dialog"
    aria-modal="true"
    :aria-label="t('serialDebug.conn.settings')"
    @click.self="close"
  >
    <div
      class="ty-card max-h-[calc(100dvh-2rem)] w-[min(90vw,400px)] overflow-y-auto shadow-[var(--ty-overlay-shadow)]"
    >
      <div
        class="flex items-center justify-between border-b border-[var(--ty-border)] px-4 py-3"
      >
        <h2 class="text-sm font-semibold text-[var(--ty-text)]">
          {{ t("serialDebug.conn.settings") }}
        </h2>
        <button
          type="button"
          class="page-header-btn flex size-8 items-center justify-center rounded-lg"
          :aria-label="t('common.closeDialog')"
          @click="close"
        >
          <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-3.5" />
        </button>
      </div>
      <div class="flex flex-col gap-4 p-4">
        <div class="grid grid-cols-3 gap-3">
          <label class="field cursor-pointer">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{
              t("serialDebug.conn.dataBits")
            }}</span>
            <TySelect
              v-model="s.dataBits"
              :aria-label="t('serialDebug.conn.dataBits')"
              :options="dataBitsOptions"
              :disabled="s.open"
            />
          </label>
          <label class="field cursor-pointer">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{
              t("serialDebug.conn.parity")
            }}</span>
            <TySelect
              v-model="s.parity"
              :aria-label="t('serialDebug.conn.parity')"
              :options="parityOptions"
              :disabled="s.open"
            />
          </label>
          <label class="field cursor-pointer">
            <span class="conn-field-label mb-1 text-xs font-semibold">{{
              t("serialDebug.conn.stopBits")
            }}</span>
            <TySelect
              v-model="s.stopBits"
              :aria-label="t('serialDebug.conn.stopBits')"
              :options="stopBitsOptions"
              :disabled="s.open"
            />
          </label>
        </div>

        <div class="check-row">
          <div class="check-row__copy flex-col items-start">
            <span class="text-[var(--ty-text)]">{{
              t("serialDebug.conn.autoRelease")
            }}</span>
            <p
              id="serial-setting-autoRelease-hint"
              class="text-xs leading-relaxed text-[var(--ty-text-muted)]"
            >
              {{ t("serialDebug.conn.autoReleaseTip") }}
            </p>
          </div>
          <TySwitch
            v-model="s.autoRelease"
            size="sm"
            :aria-label="t('serialDebug.conn.autoRelease')"
            aria-describedby="serial-setting-autoRelease-hint"
          />
        </div>

        <div class="check-row">
          <span class="text-[var(--ty-text)]">{{
            t("serialDebug.conn.hexView")
          }}</span>
          <TySwitch
            v-model="s.hexView"
            size="sm"
            :aria-label="t('serialDebug.conn.hexView')"
          />
        </div>

        <div class="check-row">
          <div class="check-row__copy flex-col items-start">
            <span class="text-[var(--ty-text)]">{{
              t("serialDebug.conn.ansiParse")
            }}</span>
            <p
              id="serial-setting-ansiParse-hint"
              class="text-xs leading-relaxed text-[var(--ty-text-muted)]"
            >
              {{ t("serialDebug.conn.ansiParseTip") }}
            </p>
          </div>
          <TySwitch
            v-model="s.ansiEnabled"
            size="sm"
            :aria-label="t('serialDebug.conn.ansiParse')"
            aria-describedby="serial-setting-ansiParse-hint"
          />
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.field {
  display: flex;
  flex-direction: column;
}

.check-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  font-size: 0.875rem;
}

.check-row__copy {
  display: inline-flex;
  min-width: 0;
  align-items: flex-start;
  flex-direction: column;
  gap: 0.25rem;
}
</style>
