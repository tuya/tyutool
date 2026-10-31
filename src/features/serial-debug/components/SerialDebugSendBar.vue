<script setup lang="ts">
import { computed, nextTick, onDeactivated, ref, useId, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useSerialDebugStore } from "@/stores/serial-debug";
import type { SendHistoryEntry, SendMode } from "../types";
import SerialDebugSendHistory from "./SerialDebugSendHistory.vue";
import { parseHexInput } from "../hex-format";

const s = useSerialDebugStore();
const { t } = useI18n();
const historyIndex = ref(-1);
const historyOpen = ref(false);
const historyTriggerRef = ref<HTMLButtonElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const historyId = `send-history-${useId()}`;
type Draft = { text: string; mode: SendMode; appendCrlf: boolean };
const historyDraft = ref<Draft | null>(null);
let browsing: SendHistoryEntry[] = [];
let draftInteraction = 0;
const composing = ref(false);
const sendErrorId = useId();
const previewId = useId();
const sendStatusId = useId();
const hexPreview = computed(() =>
  s.sendMode === "hex" ? parseHexInput(s.sendInput) : null,
);
const sendPreview = computed(() => {
  if (!s.sendInput) return "";
  if (hexPreview.value) {
    const { bytes, ignoredCount } = hexPreview.value;
    const count = t("serialDebug.send.byteCount", { n: bytes.length });
    const hint =
      !bytes.length && s.sendError?.kind !== "invalidHex"
        ? ` · ${t("serialDebug.send.invalidHex")}`
        : "";
    return ignoredCount
      ? `${count}${hint} · ${t("serialDebug.send.hexPreviewIgnored", { n: ignoredCount })}`
      : `${count}${hint}`;
  }
  const bytes =
    new TextEncoder().encode(s.sendInput).length + (s.sendAppendCrlf ? 2 : 0);
  return `${t("serialDebug.send.byteCount", { n: bytes })} · ${t(s.sendAppendCrlf ? "serialDebug.send.crlfPreview" : "serialDebug.send.noCrlfPreview")}`;
});
const sendStatus = computed(() =>
  s.sending
    ? t("serialDebug.send.sending")
    : !s.open
      ? t("serialDebug.send.openPortHint")
      : !s.sendInput
        ? t("serialDebug.send.emptyHint")
        : "",
);
const sendErrorText = computed(() => {
  if (s.sendError?.kind === "invalidHex")
    return t("serialDebug.send.invalidHex");
  if (s.sendError?.kind === "failed")
    return t("serialDebug.send.failed", { msg: s.sendError.message });
  return "";
});
watch(
  [() => s.sendMode, () => s.sendAppendCrlf],
  () => {
    draftInteraction++;
  },
  { flush: "sync" },
);
onDeactivated(() => {
  historyOpen.value = false;
});

function snapshot(): Draft {
  return { text: s.sendInput, mode: s.sendMode, appendCrlf: s.sendAppendCrlf };
}
function applyEntry(entry: SendHistoryEntry): void {
  draftInteraction++;
  s.sendInput = entry.text;
  if (entry.mode !== null && entry.appendCrlf !== null) {
    s.sendMode = entry.mode;
    s.sendAppendCrlf = entry.appendCrlf;
  }
}
function restoreDraft(): void {
  if (!historyDraft.value) return;
  draftInteraction++;
  s.sendInput = historyDraft.value.text;
  s.sendMode = historyDraft.value.mode;
  s.sendAppendCrlf = historyDraft.value.appendCrlf;
  historyDraft.value = null;
  historyIndex.value = -1;
  browsing = [];
  void nextTick(() => inputRef.value?.focus());
}
async function submit(): Promise<void> {
  const interaction = draftInteraction;
  if ((await s.send()) && interaction === draftInteraction) {
    historyIndex.value = -1;
    historyDraft.value = null;
    browsing = [];
  }
}
function onDraftInput(): void {
  draftInteraction++;
  historyIndex.value = -1;
  browsing = [];
}
function onKey(ev: KeyboardEvent): void {
  if (composing.value || ev.isComposing || ev.keyCode === 229) return;
  if (ev.key === "Enter") {
    ev.preventDefault();
    void submit();
  } else if (ev.key === "ArrowUp") {
    if (s.sendHistory.length === 0) return;
    ev.preventDefault();
    if (historyIndex.value === -1) {
      historyDraft.value = snapshot();
      browsing = [...s.sendHistory];
    }
    historyIndex.value = Math.min(historyIndex.value + 1, browsing.length - 1);
    const entry = browsing[historyIndex.value];
    if (entry) applyEntry(entry);
  } else if (ev.key === "ArrowDown" && historyIndex.value !== -1) {
    ev.preventDefault();
    if (historyIndex.value === 0) restoreDraft();
    else {
      historyIndex.value--;
      const entry = browsing[historyIndex.value];
      if (entry) applyEntry(entry);
    }
  } else if (ev.key === "Escape" && historyDraft.value) {
    ev.preventDefault();
    restoreDraft();
  }
}
function selectHistory(entry: SendHistoryEntry): void {
  historyDraft.value ??= snapshot();
  applyEntry(entry);
  closeHistory(false);
  historyIndex.value = -1;
  browsing = [];
  void nextTick(() => inputRef.value?.focus());
}
function closeHistory(restoreFocus: boolean): void {
  historyOpen.value = false;
  if (restoreFocus) void nextTick(() => historyTriggerRef.value?.focus());
}
function onHistoryKey(event: KeyboardEvent): void {
  if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    historyOpen.value = true;
  }
}
</script>

<template>
  <div
    class="send-bar flex flex-wrap items-center gap-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-3"
  >
    <div
      class="mode-toggle flex overflow-hidden rounded-lg border border-[var(--ty-border)]"
    >
      <button
        type="button"
        :class="{ active: s.sendMode === 'ascii' }"
        :aria-pressed="s.sendMode === 'ascii'"
        class="mode-btn"
        @click="s.sendMode = 'ascii'"
      >
        ASCII
      </button>
      <button
        type="button"
        :class="{ active: s.sendMode === 'hex' }"
        :aria-pressed="s.sendMode === 'hex'"
        class="mode-btn"
        @click="s.sendMode = 'hex'"
      >
        Hex
      </button>
    </div>
    <div class="min-w-[8rem] flex-1">
      <input
        ref="inputRef"
        type="text"
        class="input w-full"
        :aria-label="t('serialDebug.send.inputLabel')"
        :aria-invalid="s.sendError?.kind === 'invalidHex'"
        :aria-describedby="`${previewId} ${sendStatusId}${sendErrorText ? ` ${sendErrorId}` : ''}`"
        :placeholder="t('serialDebug.send.placeholder')"
        v-model="s.sendInput"
        @input="onDraftInput"
        @keydown="onKey"
        @compositionstart="composing = true"
        @compositionend="composing = false"
      />
    </div>

    <div class="history-wrap">
      <button
        ref="historyTriggerRef"
        type="button"
        class="btn-icon"
        :aria-label="t('serialDebug.send.history')"
        :aria-expanded="historyOpen"
        :aria-controls="historyOpen ? historyId : undefined"
        @keydown="onHistoryKey"
        @click="historyOpen = !historyOpen"
      >
        <FontAwesomeIcon :icon="['fas', 'clock-rotate-left']" />
      </button>
      <SerialDebugSendHistory
        v-if="historyOpen"
        :id="historyId"
        :trigger="historyTriggerRef"
        @close="closeHistory"
        @fill="selectHistory"
      />
    </div>
    <button
      v-if="historyDraft"
      type="button"
      class="btn-icon restore-draft"
      @click="restoreDraft"
    >
      {{ t("serialDebug.history.restoreDraft") }}
    </button>

    <label class="check-row flex cursor-pointer items-center gap-1 text-xs">
      <input
        type="checkbox"
        class="shrink-0"
        v-model="s.sendAppendCrlf"
        :disabled="s.sendMode === 'hex'"
      />
      <span>\r\n</span>
    </label>
    <button
      type="button"
      class="btn-primary"
      :disabled="!s.open || !s.sendInput || s.sending"
      :aria-busy="s.sending"
      :aria-describedby="sendStatusId"
      :title="sendStatus || undefined"
      @click="submit"
    >
      {{
        t(s.sending ? "serialDebug.send.sending" : "serialDebug.send.sendBtn")
      }}
    </button>
    <div
      class="send-feedback flex min-h-4 w-full flex-wrap items-center gap-x-3 gap-y-1 text-xs text-[var(--ty-text-muted)]"
    >
      <span
        :id="previewId"
        class="send-preview"
        :class="{ 'text-[var(--ty-warning)]': hexPreview?.ignoredCount }"
        >{{ sendPreview }}</span
      >
      <span :id="sendStatusId" role="status">{{ sendStatus }}</span>
      <p
        v-if="sendErrorText"
        :id="sendErrorId"
        role="alert"
        class="w-full text-[var(--ty-danger)]"
      >
        {{ sendErrorText }}
      </p>
    </div>
  </div>
</template>

<style scoped>
.input {
  border: 1px solid var(--ty-border-strong);
  background: var(--ty-field);
  border-radius: 0.5rem;
  padding: 0.5rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.mode-btn {
  padding: 0.375rem 0.75rem;
  font-size: 0.8rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    background-color 0.15s ease,
    color 0.15s ease;
}
.mode-btn:not(.active):hover {
  background: var(--ty-surface-muted);
}
.mode-btn.active {
  background: var(--ty-primary-soft);
  color: var(--ty-primary);
}
/* Parent .mode-toggle has overflow:hidden, which clips the global box-shadow
   focus ring — use an inset outline so keyboard focus stays visible. */
.mode-btn:focus-visible {
  outline: 2px solid var(--ty-ring);
  outline-offset: -2px;
  box-shadow: none;
}
.btn-primary {
  padding: 0.5rem 1rem;
  background: var(--ty-primary);
  color: var(--ty-on-primary);
  border-radius: 0.5rem;
  font-weight: 600;
  cursor: pointer;
  transition: opacity 0.15s ease;
}
.btn-primary:hover:not(:disabled) {
  opacity: 0.88;
}
.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.btn-icon {
  padding: 0.5rem 0.625rem;
  border: 1px solid var(--ty-border);
  border-radius: 0.5rem;
  background: var(--ty-canvas);
  cursor: pointer;
  transition: background-color 0.15s ease;
}
.btn-icon:hover:not(:disabled) {
  background: var(--ty-surface-muted);
}
.btn-icon:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}
.restore-draft {
  font-size: 0.75rem;
  white-space: nowrap;
}
</style>
