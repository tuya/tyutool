<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import { useI18n } from "vue-i18n";
import { useSerialDebugStore } from "@/stores/serial-debug";
import TySelect from "@/components/TySelect.vue";
import TySwitch from "@/components/TySwitch.vue";

const emit = defineEmits<{ close: [] }>();
const s = useSerialDebugStore();
const { t } = useI18n();

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

function onKeydown(e: KeyboardEvent): void {
  if (e.key === "Escape") emit("close");
}
onMounted(() => window.addEventListener("keydown", onKeydown));
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
    role="dialog"
    aria-modal="true"
    :aria-label="t('serialDebug.conn.settings')"
    @click.self="emit('close')"
  >
    <div class="ty-card w-[min(90vw,400px)] overflow-hidden">
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
          @click="emit('close')"
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
              :options="stopBitsOptions"
              :disabled="s.open"
            />
          </label>
        </div>

        <div class="check-row">
          <div class="check-row__copy">
            <span class="text-[var(--ty-text)]">{{
              t("serialDebug.conn.autoRelease")
            }}</span>
            <FontAwesomeIcon
              :icon="['fas', 'circle-info']"
              class="size-3.5 cursor-help text-[var(--ty-text-muted)]"
              :title="t('serialDebug.conn.autoReleaseTip')"
            />
          </div>
          <TySwitch
            v-model="s.autoRelease"
            size="sm"
            :aria-label="t('serialDebug.conn.autoRelease')"
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
          <div class="check-row__copy">
            <span class="text-[var(--ty-text)]">{{
              t("serialDebug.conn.ansiParse")
            }}</span>
            <FontAwesomeIcon
              :icon="['fas', 'circle-info']"
              class="size-3.5 cursor-help text-[var(--ty-text-muted)]"
              :title="t('serialDebug.conn.ansiParseTip')"
            />
          </div>
          <TySwitch
            v-model="s.ansiEnabled"
            size="sm"
            :aria-label="t('serialDebug.conn.ansiParse')"
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
  align-items: center;
  gap: 0.5rem;
}
</style>
