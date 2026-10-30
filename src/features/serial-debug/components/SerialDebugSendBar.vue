<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, useId, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useSerialDebugStore } from "@/stores/serial-debug";

const s = useSerialDebugStore();
const { t } = useI18n();
const historyIndex = ref(-1);
const historyOpen = ref(false);
const historyWrapRef = ref<HTMLElement | null>(null);
const historyTriggerRef = ref<HTMLButtonElement | null>(null);
const inputRef = ref<HTMLInputElement | null>(null);
const historyId = `send-history-${useId()}`;
let historyDraft = "";
let draftInteraction = 0;
const composing = ref(false);
const sendErrorId = useId();
const sendErrorText = computed(() => {
  if (s.sendError?.kind === "invalidHex")
    return t("serialDebug.send.invalidHex");
  if (s.sendError?.kind === "failed") {
    return t("serialDebug.send.failed", { msg: s.sendError.message });
  }
  return "";
});

function onDocMousedown(e: MouseEvent): void {
  if (!historyWrapRef.value?.contains(e.target as Node)) {
    historyOpen.value = false;
  }
}

watch(historyOpen, (open) => {
  if (open) document.addEventListener("mousedown", onDocMousedown);
  else document.removeEventListener("mousedown", onDocMousedown);
});
onUnmounted(() => {
  document.removeEventListener("mousedown", onDocMousedown);
});

async function submit(): Promise<void> {
  const interaction = draftInteraction;
  if ((await s.send()) && interaction === draftInteraction) {
    historyIndex.value = -1;
    historyDraft = "";
  }
}
function onDraftInput(): void {
  draftInteraction++;
  historyIndex.value = -1;
}

function onKey(ev: KeyboardEvent): void {
  if (composing.value || ev.isComposing || ev.keyCode === 229) return;
  if (ev.key === "Enter") {
    ev.preventDefault();
    void submit();
    return;
  }
  if (ev.key === "ArrowUp") {
    if (s.sendHistory.length === 0) return;
    ev.preventDefault();
    draftInteraction++;
    if (historyIndex.value === -1) historyDraft = s.sendInput;
    historyIndex.value = Math.min(
      historyIndex.value + 1,
      s.sendHistory.length - 1,
    );
    s.sendInput = s.sendHistory[historyIndex.value] ?? "";
  } else if (ev.key === "ArrowDown") {
    if (historyIndex.value === -1) return;
    ev.preventDefault();
    draftInteraction++;
    if (historyIndex.value <= 0) {
      historyIndex.value = -1;
      s.sendInput = historyDraft;
      return;
    }
    ev.preventDefault();
    historyIndex.value -= 1;
    s.sendInput = s.sendHistory[historyIndex.value] ?? "";
  }
}

function selectHistory(item: string): void {
  draftInteraction++;
  s.sendInput = item;
  historyOpen.value = false;
  historyIndex.value = -1;
  void nextTick(() => inputRef.value?.focus());
}

function onHistoryKey(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    historyOpen.value = false;
    historyTriggerRef.value?.focus();
  } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    if (!historyOpen.value) {
      historyOpen.value = true;
      void nextTick(() =>
        historyWrapRef.value
          ?.querySelector<HTMLButtonElement>(".history-item")
          ?.focus(),
      );
    } else {
      const items = Array.from(
        historyWrapRef.value?.querySelectorAll<HTMLButtonElement>(
          ".history-item",
        ) ?? [],
      );
      const index = items.indexOf(document.activeElement as HTMLButtonElement);
      items[
        Math.max(
          0,
          Math.min(
            items.length - 1,
            index + (event.key === "ArrowDown" ? 1 : -1),
          ),
        )
      ]?.focus();
    }
  }
}

function onHistoryFocusout(event: FocusEvent): void {
  if (!historyWrapRef.value?.contains(event.relatedTarget as Node))
    historyOpen.value = false;
}
</script>

<template>
  <div
    class="send-bar flex items-center gap-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-2"
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
    <div class="min-w-0 flex-1">
      <input
        ref="inputRef"
        type="text"
        class="input w-full"
        :aria-label="t('serialDebug.send.inputLabel')"
        :aria-invalid="s.sendError?.kind === 'invalidHex'"
        :aria-describedby="sendErrorText ? sendErrorId : undefined"
        :placeholder="t('serialDebug.send.placeholder')"
        :disabled="!s.open"
        v-model="s.sendInput"
        @input="onDraftInput"
        @keydown="onKey"
        @compositionstart="composing = true"
        @compositionend="composing = false"
      />
      <p v-if="sendErrorText" :id="sendErrorId" role="alert">
        {{ sendErrorText }}
      </p>
    </div>

    <!-- history button + dropdown -->
    <div
      ref="historyWrapRef"
      class="history-wrap relative"
      @keydown="onHistoryKey"
      @focusout="onHistoryFocusout"
    >
      <button
        ref="historyTriggerRef"
        type="button"
        class="btn-icon"
        :aria-label="t('serialDebug.send.history')"
        :aria-expanded="historyOpen"
        :aria-controls="historyOpen ? historyId : undefined"
        :disabled="s.sendHistory.length === 0"
        @click="historyOpen = !historyOpen"
      >
        <FontAwesomeIcon :icon="['fas', 'clock-rotate-left']" />
      </button>
      <div
        v-if="historyOpen"
        :id="historyId"
        role="group"
        :aria-label="t('serialDebug.send.history')"
        class="history-dropdown absolute bottom-full right-0 z-50 mb-1 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface)] shadow-lg"
      >
        <button
          v-for="(item, i) in s.sendHistory"
          :key="i"
          type="button"
          class="history-item"
          :title="item"
          @click="selectHistory(item)"
        >
          {{ item }}
        </button>
      </div>
    </div>

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
      @click="submit"
    >
      {{ t("serialDebug.send.sendBtn") }}
    </button>
  </div>
</template>

<style scoped>
.input {
  border: 1px solid var(--ty-border);
  background: var(--ty-canvas);
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
  background: var(--ty-primary);
  color: var(--ty-on-primary);
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
.history-dropdown {
  min-width: 14rem;
  max-height: 16rem;
  overflow-y: auto;
}
.history-item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 0.375rem 0.75rem;
  font-size: 0.8125rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}
.history-item:focus-visible {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  overflow: visible;
}
.history-item:hover {
  background: var(--ty-surface-muted);
}
</style>
