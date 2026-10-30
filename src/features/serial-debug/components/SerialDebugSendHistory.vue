<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useFloatingPanel } from "@/composables/useFloatingPanel";
import { useSerialDebugStore } from "@/stores/serial-debug";
import { sendHistoryKey } from "../send-history";
import { MAX_FAVORITE_COMMANDS } from "../constants";
import type { SendHistoryEntry } from "../types";

const props = defineProps<{ id: string; trigger: HTMLElement | null }>();
const emit = defineEmits<{
  close: [restoreFocus: boolean];
  fill: [entry: SendHistoryEntry];
}>();
const s = useSerialDebugStore();
const { t, locale } = useI18n();
const panel = ref<HTMLElement | null>(null);
const queryInput = ref<HTMLInputElement | null>(null);
const query = ref("");
const favoritesOnly = ref(false);
const selectedKey = ref("");
const name = ref("");
const notice = ref("");
const visible = computed(() => {
  const needle = query.value.trim().toLocaleLowerCase();
  return s.sendHistory.filter(
    (entry) =>
      (!favoritesOnly.value || entry.favorite) &&
      `${entry.text} ${entry.name}`.toLocaleLowerCase().includes(needle),
  );
});
const selected = computed(
  () =>
    visible.value.find(
      (entry) => sendHistoryKey(entry) === selectedKey.value,
    ) ?? null,
);
watch(
  visible,
  (entries) => {
    if (!entries.some((entry) => sendHistoryKey(entry) === selectedKey.value))
      selectedKey.value = entries[0] ? sendHistoryKey(entries[0]) : "";
  },
  { immediate: true },
);
watch(
  [selectedKey, () => selected.value?.name],
  () => {
    name.value = selected.value?.name ?? "";
  },
  { immediate: true },
);
const { style, position } = useFloatingPanel(
  panel,
  () => true,
  () => props.trigger?.getBoundingClientRect() ?? null,
);
watch([visible, selected], () => {
  void nextTick(position);
});

function settings(entry: SendHistoryEntry): string {
  if (entry.mode === null) return t("serialDebug.history.unknownSettings");
  if (entry.mode === "hex") return "Hex";
  return `ASCII · ${entry.appendCrlf ? "CRLF" : t("serialDebug.history.noNewline")}`;
}
function date(entry: SendHistoryEntry): string {
  return entry.lastSentAt === null
    ? t("serialDebug.history.unknownTime")
    : new Intl.DateTimeFormat(locale.value, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      }).format(entry.lastSentAt);
}
function select(entry: SendHistoryEntry): void {
  selectedKey.value = sendHistoryKey(entry);
  notice.value = "";
}
function favorite(): void {
  if (
    selected.value &&
    !s.toggleFavoriteCommand(sendHistoryKey(selected.value))
  )
    notice.value = t("serialDebug.history.favoriteLimit", {
      count: MAX_FAVORITE_COMMANDS,
    });
}
function remove(): void {
  if (!selected.value) return;
  s.deleteHistoryCommand(sendHistoryKey(selected.value));
  notice.value = t("serialDebug.history.deleted");
  void nextTick(() => queryInput.value?.focus());
}
function saveName(): void {
  if (selected.value)
    s.renameHistoryCommand(sendHistoryKey(selected.value), name.value);
  notice.value = t("serialDebug.history.nameSaved");
}
function onNameKey(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.isComposing || event.keyCode === 229)
    return;
  event.preventDefault();
  saveName();
}
function onKey(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return;
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    emit("close", true);
  } else if (event.key === "Enter" && event.target === queryInput.value) {
    event.preventDefault();
    if (selected.value) emit("fill", selected.value);
  } else if (
    (event.key === "ArrowUp" || event.key === "ArrowDown") &&
    (event.target === queryInput.value ||
      (event.target as HTMLElement).classList.contains("history-item"))
  ) {
    event.preventDefault();
    const index = visible.value.findIndex(
      (entry) => sendHistoryKey(entry) === selectedKey.value,
    );
    const entry =
      visible.value[
        Math.max(
          0,
          Math.min(
            visible.value.length - 1,
            index + (event.key === "ArrowDown" ? 1 : -1),
          ),
        )
      ];
    if (entry) select(entry);
    void nextTick(() =>
      panel.value
        ?.querySelector<HTMLElement>('.history-item[aria-pressed="true"]')
        ?.scrollIntoView?.({ block: "nearest" }),
    );
  }
}
function outside(event: MouseEvent): void {
  if (
    !panel.value?.contains(event.target as Node) &&
    !props.trigger?.contains(event.target as Node)
  )
    emit("close", false);
}
onMounted(() => {
  queryInput.value?.focus();
  document.addEventListener("mousedown", outside);
});
onUnmounted(() => document.removeEventListener("mousedown", outside));
</script>

<template>
  <Teleport to="body">
    <section
      ref="panel"
      :id="id"
      class="history-dropdown"
      :style="style"
      role="region"
      :aria-label="t('serialDebug.send.history')"
      @keydown="onKey"
      @focusout="
        (event) => {
          if (
            event.relatedTarget &&
            !panel?.contains(event.relatedTarget as Node) &&
            event.relatedTarget !== trigger
          )
            emit('close', false);
        }
      "
    >
      <div class="history-heading">
        <strong>{{ t("serialDebug.send.history") }}</strong>
        <button
          type="button"
          :aria-pressed="favoritesOnly"
          @click="favoritesOnly = !favoritesOnly"
        >
          {{ t("serialDebug.history.favoritesOnly") }}
        </button>
        <button
          type="button"
          :aria-label="t('serialDebug.history.close')"
          @click="emit('close', true)"
        >
          {{ t("serialDebug.history.close") }}
        </button>
      </div>
      <input
        ref="queryInput"
        v-model="query"
        type="search"
        class="history-search"
        :aria-label="t('serialDebug.history.search')"
        :placeholder="t('serialDebug.history.search')"
      />
      <div class="history-layout">
        <div class="history-list">
          <p v-if="!visible.length" class="history-empty">
            {{
              t(
                query || favoritesOnly
                  ? "serialDebug.history.noMatch"
                  : "serialDebug.history.empty",
              )
            }}
          </p>
          <button
            v-for="entry in visible"
            :key="sendHistoryKey(entry)"
            type="button"
            class="history-item"
            :aria-pressed="selected === entry"
            :title="entry.text"
            @click="select(entry)"
          >
            <span class="history-command">{{ entry.name || entry.text }}</span>
            <span class="history-meta"
              >{{
                entry.favorite ? t("serialDebug.history.favorite") + " · " : ""
              }}{{ settings(entry) }} · {{ date(entry) }}</span
            >
          </button>
        </div>
        <div v-if="selected" class="history-preview">
          <div class="preview-heading">
            <strong>{{ t("serialDebug.history.preview") }}</strong
            ><span>{{ settings(selected) }}</span>
          </div>
          <pre>{{ selected.text }}</pre>
          <p class="history-meta">
            {{
              t("serialDebug.history.lastSent", {
                time: date(selected),
                count: selected.sendCount,
              })
            }}
          </p>
          <div class="history-name">
            <input
              v-model="name"
              maxlength="80"
              :aria-label="t('serialDebug.history.name')"
              :placeholder="t('serialDebug.history.name')"
              @keydown="onNameKey"
            />
            <button
              type="button"
              :disabled="name.trim() === selected.name"
              @click="saveName"
            >
              {{ t("serialDebug.history.saveName") }}
            </button>
          </div>
          <div class="history-actions">
            <button
              type="button"
              :aria-pressed="selected.favorite"
              @click="favorite"
            >
              {{
                t(
                  selected.favorite
                    ? "serialDebug.history.unfavorite"
                    : "serialDebug.history.favorite",
                )
              }}
            </button>
            <button type="button" @click="remove">
              {{ t("serialDebug.history.delete") }}
            </button>
          </div>
          <button
            type="button"
            class="history-fill"
            @click="emit('fill', selected)"
          >
            {{ t("serialDebug.history.fill") }}
          </button>
          <p class="history-meta">
            {{
              t(
                selected.mode === null
                  ? "serialDebug.history.legacyHint"
                  : "serialDebug.history.fillHint",
              )
            }}
          </p>
        </div>
      </div>
      <div class="history-footer">
        <span>{{ t("serialDebug.history.keys") }}</span
        ><span role="status">{{ notice }}</span>
      </div>
    </section>
  </Teleport>
</template>

<style scoped>
.history-dropdown {
  position: fixed;
  z-index: 50;
  width: 44rem;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--ty-border);
  border-radius: 0.75rem;
  background: var(--ty-surface);
  color: var(--ty-text);
  box-shadow: 0 12px 40px #0003;
  overflow: auto;
  font-size: 0.8125rem;
}
.history-heading,
.preview-heading,
.history-actions,
.history-name {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}
.history-heading {
  padding: 0.75rem;
}
.history-heading strong {
  flex: 1;
}
button {
  border: 1px solid var(--ty-border);
  border-radius: 0.375rem;
  background: var(--ty-canvas);
  padding: 0.35rem 0.6rem;
  cursor: pointer;
}
button:hover {
  background: var(--ty-surface-muted);
}
button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
button[aria-pressed="true"] {
  border-color: var(--ty-primary);
}
input {
  border: 1px solid var(--ty-border);
  border-radius: 0.375rem;
  background: var(--ty-canvas);
  padding: 0.5rem;
  min-width: 0;
}
.history-search {
  margin: 0 0.75rem 0.75rem;
}
.history-layout {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  min-height: 0;
  border-top: 1px solid var(--ty-border);
}
.history-list {
  overflow: auto;
  max-height: 21rem;
  padding: 0.5rem;
  border-right: 1px solid var(--ty-border);
}
.history-item {
  display: block;
  width: 100%;
  text-align: left;
  border-color: transparent;
  margin-bottom: 0.25rem;
}
.history-command {
  display: block;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.history-item:focus-visible .history-command {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.history-meta {
  display: block;
  color: var(--ty-text-muted);
  font-size: 0.75rem;
  line-height: 1.5;
}
.history-preview {
  display: flex;
  flex-direction: column;
  gap: 0.6rem;
  padding: 0.75rem;
  min-width: 0;
}
.preview-heading {
  justify-content: space-between;
  flex-wrap: wrap;
}
.history-preview pre {
  max-height: 8rem;
  overflow: auto;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  margin: 0;
  padding: 0.6rem;
  border-radius: 0.375rem;
  background: var(--ty-canvas);
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.history-name input {
  flex: 1;
  width: 0;
}
.history-fill {
  background: var(--ty-primary);
  color: var(--ty-on-primary);
  margin-top: auto;
}
.history-fill:hover {
  background: var(--ty-primary);
  filter: brightness(0.95);
}
.history-footer {
  padding: 0.6rem 0.75rem;
  border-top: 1px solid var(--ty-border);
  color: var(--ty-text-muted);
  font-size: 0.75rem;
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}
.history-empty {
  color: var(--ty-text-muted);
  padding: 1rem;
}
@media (max-width: 640px) {
  .history-layout {
    grid-template-columns: 1fr;
  }
  .history-list {
    max-height: 8rem;
    border-right: 0;
    border-bottom: 1px solid var(--ty-border);
  }
}
</style>
