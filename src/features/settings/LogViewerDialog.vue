<script setup lang="ts">
import { ref, watch, computed, nextTick, useId, onBeforeUnmount } from "vue";
import { useCopyFeedback } from "@/composables/useCopyFeedback";
import { useModalFocus } from "@/composables/useModalFocus";
import { useI18n } from "vue-i18n";
import { isTauriRuntime } from "@/runtime";
import { exportLogsAndReport } from "./report-issue";
import {
  listLogFileOpeners,
  openLogFileInEditor,
  type LogFileOpener,
} from "./log-openers";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();
const dialogRef = ref<HTMLElement | null>(null);
const openerDialogRef = ref<HTMLElement | null>(null);
const titleId = `log-viewer-${useId()}`;
const openerTitleId = `log-opener-${useId()}`;
useModalFocus(
  dialogRef,
  () => props.open,
  () => emit("close"),
);

const { t } = useI18n();

// ── Types ─────────────────────────────────────────────────────────────────────

// Mirrors src-tauri/src/lib.rs LogFileInfo
interface LogFileInfo {
  name: string;
  sizeBytes: number;
  modifiedMs: number;
}

type LogLevel = "error" | "warn" | "info" | "debug" | "trace" | "default";

// ── View state ─────────────────────────────────────────────────────────────────

const view = ref<"list" | "content">("list");

// ── File list state ────────────────────────────────────────────────────────────

const fileList = ref<LogFileInfo[]>([]);
const listLoading = ref(false);
const listError = ref("");
let listGeneration = 0;
let contentGeneration = 0;
let openerGeneration = 0;

async function loadFileList(): Promise<void> {
  const generation = ++listGeneration;
  if (!isTauriRuntime()) {
    fileList.value = [];
    return;
  }
  listLoading.value = true;
  listError.value = "";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const files = await invoke<LogFileInfo[]>("list_log_files");
    if (generation === listGeneration && props.open) fileList.value = files;
  } catch (e) {
    if (generation === listGeneration && props.open)
      listError.value = String(e);
  } finally {
    if (generation === listGeneration && props.open) listLoading.value = false;
  }
}

// ── Content state ──────────────────────────────────────────────────────────────

const MAX_TAIL_BYTES = 256 * 1024;
const LINES_PER_PAGE = 300;
const pageIndex = ref(0);

const selectedFile = ref<LogFileInfo | null>(null);
const rawContent = ref("");
const contentLoading = ref(false);
const contentError = ref("");
const isTruncated = ref(false);
const contentEl = ref<HTMLElement | null>(null);

async function openFile(file: LogFileInfo, refresh = false): Promise<void> {
  if (!isTauriRuntime()) return;
  const generation = ++contentGeneration;
  const current = () =>
    generation === contentGeneration && props.open && view.value === "content";
  const oldOffset = contentEl.value?.scrollTop ?? 0;
  const oldPage = pageIndex.value;
  const follow =
    !refresh ||
    !contentEl.value ||
    (pageIndex.value === pageCount.value - 1 &&
      contentEl.value.scrollHeight - oldOffset - contentEl.value.clientHeight <=
        2);
  selectedFile.value = file;
  if (!refresh) {
    searchQuery.value = "";
    currentMatchIndex.value = 0;
    rawContent.value = "";
  }
  contentError.value = "";
  if (!refresh) isTruncated.value = false;
  contentLoading.value = true;
  view.value = "content";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    const content = await invoke<string>("read_log_tail", {
      maxBytes: MAX_TAIL_BYTES,
      filename: file.name,
    });
    if (!current()) return;
    rawContent.value = content;
    pageIndex.value = follow
      ? pageCount.value - 1
      : Math.min(oldPage, pageCount.value - 1);
    currentMatchIndex.value = clampIndex(currentMatchIndex.value);
    isTruncated.value = file.sizeBytes > MAX_TAIL_BYTES;
  } catch (e) {
    if (current()) contentError.value = String(e);
  } finally {
    if (current()) {
      contentLoading.value = false;
      await nextTick();
      if (current() && contentEl.value)
        contentEl.value.scrollTop = follow
          ? contentEl.value.scrollHeight
          : oldOffset;
    }
  }
}

function backToList(): void {
  contentGeneration++;
  contentLoading.value = false;
  view.value = "list";
  searchQuery.value = "";
  void nextTick(() => {
    const row = Array.from(
      dialogRef.value?.querySelectorAll<HTMLButtonElement>(
        ".log-file-button",
      ) ?? [],
    ).find((button) => button.dataset.filename === selectedFile.value?.name);
    row?.focus();
  });
}

// ── Search state ───────────────────────────────────────────────────────────────

const searchQuery = ref("");
const currentMatchIndex = ref(0);

// ── Log line processing ────────────────────────────────────────────────────────

const LEVEL_RE = /\b(ERROR|WARN|INFO|DEBUG|TRACE)\b/;

function getLineLevel(line: string): LogLevel {
  const m = LEVEL_RE.exec(line);
  if (!m) return "default";
  return m[1].toLowerCase() as LogLevel;
}

const LEVEL_CLASS: Record<LogLevel, string> = {
  error: "text-[var(--ty-danger)]",
  warn: "text-[var(--ty-warning)]",
  info: "text-[var(--ty-text)]",
  debug: "text-[var(--ty-text-muted)]",
  trace: "text-[var(--ty-text-muted)]",
  default: "text-[var(--ty-text)]",
};

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function highlightMatch(raw: string, matcher: RegExp): string {
  let html = "";
  let start = 0;
  // Match source text before escaping; HTML entities are display encoding.
  for (const match of raw.matchAll(matcher)) {
    html += escapeHtml(raw.slice(start, match.index));
    html += `<mark class="log-search-highlight">${escapeHtml(match[0])}</mark>`;
    start = match.index + match[0].length;
  }
  return html + escapeHtml(raw.slice(start));
}

// Parsing and case normalization depend on file content, not each search edit.
const preparedLines = computed(() =>
  rawContent.value
    ? rawContent.value.split("\n").map((raw, originalIndex) => ({
        raw,
        lowerText: raw.toLowerCase(),
        level: getLineLevel(raw),
        html: escapeHtml(raw),
        originalIndex,
      }))
    : [],
);
const filteredLines = computed(() => {
  const needle = searchQuery.value.toLowerCase();
  return needle
    ? preparedLines.value.filter((line) => line.lowerText.includes(needle))
    : preparedLines.value;
});
const pageCount = computed(() =>
  Math.max(1, Math.ceil(filteredLines.value.length / LINES_PER_PAGE)),
);
const pageStart = computed(() => pageIndex.value * LINES_PER_PAGE);
const displayLines = computed(() => {
  const q = searchQuery.value;
  const matcher = q
    ? new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi")
    : null;
  return filteredLines.value
    .slice(pageStart.value, pageStart.value + LINES_PER_PAGE)
    .map((line) => ({
      level: line.level,
      html: matcher ? highlightMatch(line.raw, matcher) : line.html,
      originalIndex: line.originalIndex,
    }));
});

const matchTotal = computed(() => filteredLines.value.length);

async function changePage(delta: number): Promise<void> {
  pageIndex.value = Math.max(
    0,
    Math.min(pageCount.value - 1, pageIndex.value + delta),
  );
  if (searchQuery.value) currentMatchIndex.value = pageStart.value;
  await nextTick();
  if (contentEl.value) contentEl.value.scrollTop = 0;
}

function clampIndex(idx: number): number {
  if (matchTotal.value === 0) return 0;
  return ((idx % matchTotal.value) + matchTotal.value) % matchTotal.value;
}

function jumpNext(): void {
  currentMatchIndex.value = clampIndex(currentMatchIndex.value + 1);
  scrollToMatch(currentMatchIndex.value);
}

function jumpPrev(): void {
  currentMatchIndex.value = clampIndex(currentMatchIndex.value - 1);
  scrollToMatch(currentMatchIndex.value);
}

function scrollToMatch(idx: number): void {
  pageIndex.value = Math.floor(idx / LINES_PER_PAGE);
  nextTick(() => {
    const el =
      contentEl.value?.querySelectorAll<HTMLElement>("[data-line]")[
        idx - pageStart.value
      ];
    el?.scrollIntoView({ block: "center" });
  });
}

watch(searchQuery, () => {
  currentMatchIndex.value = 0;
  pageIndex.value = 0;
  if (searchQuery.value) scrollToMatch(0);
});

// ── Copy ───────────────────────────────────────────────────────────────────────

const {
  state: copyState,
  message: copyMessage,
  copy: copyText,
  reset: resetCopy,
} = useCopyFeedback();
const copied = computed(() => copyState.value === "copied");
function copy(): void {
  void copyText(rawContent.value);
}
watch(selectedFile, resetCopy);

// ── Open with ─────────────────────────────────────────────────────────────────

const openerDialogOpen = ref(false);
useModalFocus(
  openerDialogRef,
  () => props.open && openerDialogOpen.value,
  () => {
    openerDialogOpen.value = false;
  },
);
const openersLoading = ref(false);
const openers = ref<LogFileOpener[]>([]);
const openersError = ref("");
const openingEditorId = ref("");

function displayOpenerLabel(opener: LogFileOpener): string {
  if (opener.id === "default") {
    return t("settings.logViewer.systemDefault");
  }
  return opener.label;
}

async function showOpenWithDialog(): Promise<void> {
  if (!selectedFile.value) return;
  openerDialogOpen.value = true;
  openersLoading.value = true;
  openersError.value = "";
  const generation = ++openerGeneration;
  try {
    const result = await listLogFileOpeners();
    if (generation === openerGeneration && props.open && openerDialogOpen.value)
      openers.value = result;
  } catch (e) {
    if (
      generation !== openerGeneration ||
      !props.open ||
      !openerDialogOpen.value
    )
      return;
    openers.value = [];
    openersError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (generation === openerGeneration) openersLoading.value = false;
  }
}

async function openWith(openerId: string): Promise<void> {
  if (!selectedFile.value || openingEditorId.value) return;
  const generation = ++openerGeneration;
  openingEditorId.value = openerId;
  openersError.value = "";
  try {
    await openLogFileInEditor(selectedFile.value.name, openerId);
    if (generation === openerGeneration && props.open)
      openerDialogOpen.value = false;
  } catch (e) {
    if (
      generation !== openerGeneration ||
      !props.open ||
      !openerDialogOpen.value
    )
      return;
    openersError.value = e instanceof Error ? e.message : String(e);
  } finally {
    if (generation === openerGeneration) openingEditorId.value = "";
  }
}

watch(openerDialogOpen, (open) => {
  if (!open) {
    openerGeneration++;
    openersLoading.value = false;
    openingEditorId.value = "";
  }
});

// ── Utils ──────────────────────────────────────────────────────────────────────

function formatSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return (bytes / 1024 / 1024).toFixed(1) + " MB";
  return Math.ceil(bytes / 1024) + " KB";
}

function formatDate(ms: number): string {
  if (ms === 0) return "—";
  return new Date(ms).toLocaleString();
}

function fileStem(name: string): string {
  return name.replace(/\.log$/, "");
}

// ── Dialog open/close ──────────────────────────────────────────────────────────

watch(
  () => props.open,
  (open) => {
    resetCopy();
    contentGeneration++;
    listGeneration++;
    openerGeneration++;
    contentLoading.value = false;
    if (open) {
      view.value = "list";
      openerDialogOpen.value = false;
      openersError.value = "";
      void loadFileList();
    }
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  contentGeneration++;
  listGeneration++;
  openerGeneration++;
});
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-[var(--ty-scrim)] p-4"
    @click.self="emit('close')"
  >
    <div
      ref="dialogRef"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="titleId"
      tabindex="-1"
      class="ty-card flex max-h-[85vh] w-full max-w-3xl flex-col gap-3 rounded-xl p-4 shadow-[var(--ty-overlay-shadow)] sm:p-5"
    >
      <p
        v-if="copyMessage"
        :role="copyState === 'failed' ? 'alert' : 'status'"
        class="text-xs"
        :class="
          copyState === 'failed'
            ? 'text-[var(--ty-danger)]'
            : 'text-[var(--ty-success)]'
        "
      >
        {{ copyMessage }}
      </p>
      <!-- ── Header ── -->
      <div class="flex items-center justify-between">
        <h2 :id="titleId" class="ty-section-title">
          {{ t("settings.logViewer.title") }}
        </h2>
        <button
          type="button"
          class="ty-btn-sm ty-btn-secondary"
          @click="emit('close')"
        >
          {{ t("settings.logViewer.close") }}
        </button>
      </div>

      <!-- ════════════════════ FILE LIST VIEW ════════════════════ -->
      <template v-if="view === 'list'">
        <!-- Loading -->
        <div
          v-if="listLoading"
          role="status"
          :aria-label="t('common.loading')"
          class="flex flex-1 items-center justify-center py-8"
        >
          <span class="loading loading-spinner loading-md opacity-60" />
        </div>

        <!-- Error -->
        <div
          v-else-if="listError"
          role="alert"
          class="rounded-lg bg-[var(--ty-field)] p-3 text-sm text-[var(--ty-danger)]"
        >
          {{ listError }}
        </div>

        <!-- Empty -->
        <div
          v-else-if="fileList.length === 0"
          class="flex flex-1 items-center justify-center py-8 text-sm text-[var(--ty-text-muted)]"
        >
          {{ t("settings.logViewer.empty") }}
        </div>

        <!-- Table -->
        <div v-else class="min-h-0 flex-1 overflow-auto">
          <table class="w-full text-sm">
            <thead>
              <tr
                class="border-b border-[var(--ty-border)] text-left text-xs text-[var(--ty-text-muted)]"
              >
                <th class="pb-1 pr-4 font-medium">
                  {{ t("settings.logViewer.fileName") }}
                </th>
                <th class="pb-1 pr-4 font-medium">
                  {{ t("settings.logViewer.size") }}
                </th>
                <th class="pb-1 font-medium">
                  {{ t("settings.logViewer.modifiedTime") }}
                </th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(file, idx) in fileList"
                :key="file.name"
                class="border-b border-[var(--ty-border)] hover:bg-[var(--ty-field)]"
              >
                <td class="py-1.5 pr-4 font-mono text-xs">
                  <button
                    type="button"
                    class="log-file-button break-all text-left hover:underline"
                    :data-filename="file.name"
                    @click="openFile(file)"
                  >
                    {{ fileStem(file.name) }}
                  </button>
                  <span
                    v-if="idx === 0"
                    class="log-current-session-badge ml-1.5 rounded px-1 py-0.5 text-[10px]"
                  >
                    {{ t("settings.logViewer.currentSession") }}
                  </span>
                </td>
                <td
                  class="py-1.5 pr-4 tabular-nums text-[var(--ty-text-muted)]"
                >
                  {{ formatSize(file.sizeBytes) }}
                </td>
                <td class="py-1.5 tabular-nums text-[var(--ty-text-muted)]">
                  {{ formatDate(file.modifiedMs) }}
                </td>
              </tr>
            </tbody>
          </table>
        </div>

        <!-- Footer -->
        <div class="flex flex-wrap gap-2">
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            :disabled="listLoading"
            @click="loadFileList"
          >
            {{ t("settings.logViewer.refresh") }}
          </button>
          <button
            type="button"
            class="ty-btn-sm ty-btn-primary-solid"
            @click="exportLogsAndReport(t)"
          >
            {{ t("settings.reportIssue.button") }}
          </button>
        </div>
      </template>

      <!-- ════════════════════ CONTENT VIEW ════════════════════ -->
      <template v-else>
        <!-- Breadcrumb -->
        <div class="flex min-w-0 items-center gap-1 text-sm">
          <button
            type="button"
            class="shrink-0 text-[var(--ty-primary)] hover:underline"
            @click="backToList"
          >
            {{ t("settings.logViewer.fileList") }}
          </button>
          <span class="text-[var(--ty-text-muted)]" aria-hidden="true">/</span>
          <span
            class="min-w-0 break-all font-mono text-xs text-[var(--ty-text-muted)]"
            >{{ fileStem(selectedFile?.name ?? "") }}</span
          >
        </div>

        <!-- Truncation notice -->
        <div
          v-if="isTruncated"
          class="log-truncated-notice rounded px-3 py-1.5 text-xs"
        >
          {{ t("settings.logViewer.truncated") }}
        </div>

        <!-- Search bar -->
        <div class="flex flex-wrap items-center gap-2">
          <input
            v-model="searchQuery"
            type="text"
            class="ty-input min-w-0 flex-1 font-mono text-xs"
            :placeholder="t('settings.logViewer.searchPlaceholder')"
            :aria-label="t('settings.logViewer.searchPlaceholder')"
          />
          <span
            v-if="searchQuery"
            class="shrink-0 text-xs text-[var(--ty-text-muted)]"
          >
            <template v-if="matchTotal === 0">{{
              t("settings.logViewer.noMatches")
            }}</template>
            <template v-else>
              {{
                t("settings.logViewer.matchCount", {
                  current: currentMatchIndex + 1,
                  total: matchTotal,
                })
              }}
            </template>
          </span>
          <button
            v-if="searchQuery"
            type="button"
            class="ty-btn-sm ty-btn-secondary px-2"
            :disabled="matchTotal === 0"
            @click="jumpPrev"
            :aria-label="t('settings.logViewer.previousMatch')"
          >
            ↑
          </button>
          <button
            v-if="searchQuery"
            type="button"
            class="ty-btn-sm ty-btn-secondary px-2"
            :disabled="matchTotal === 0"
            @click="jumpNext"
            :aria-label="t('settings.logViewer.nextMatch')"
          >
            ↓
          </button>
        </div>

        <p
          v-if="contentError"
          role="alert"
          class="text-xs text-[var(--ty-danger)]"
        >
          {{ contentError }}
        </p>
        <!-- Log content -->
        <div
          v-if="contentLoading && !rawContent"
          role="status"
          :aria-label="t('common.loading')"
          class="flex flex-1 items-center justify-center py-8"
        >
          <span class="loading loading-spinner loading-md opacity-60" />
        </div>

        <div
          v-else-if="rawContent || !contentError"
          ref="contentEl"
          :aria-busy="contentLoading"
          class="min-h-0 flex-1 overflow-auto rounded-lg bg-[var(--ty-log-bg)] p-3 font-mono text-xs leading-relaxed"
        >
          <div
            v-if="displayLines.length === 0"
            class="text-[var(--ty-text-muted)]"
          >
            {{
              searchQuery
                ? t("settings.logViewer.noMatches")
                : t("settings.logViewer.empty")
            }}
          </div>
          <div
            v-for="(line, idx) in displayLines"
            :key="line.originalIndex"
            data-line
            class="whitespace-pre-wrap break-words"
            :class="[
              LEVEL_CLASS[line.level],
              pageStart + idx === currentMatchIndex && searchQuery
                ? 'bg-[var(--ty-primary-soft)] rounded'
                : '',
            ]"
            v-html="line.html"
          />
        </div>

        <!-- Footer -->
        <div
          v-if="pageCount > 1"
          class="flex shrink-0 flex-wrap items-center gap-2 text-xs"
        >
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            :disabled="contentLoading || pageIndex === 0"
            @click="changePage(-1)"
          >
            {{ t("settings.logViewer.previousPage") }}
          </button>
          <p
            role="status"
            class="min-w-0 flex-1 tabular-nums text-[var(--ty-text-muted)]"
          >
            {{
              t("settings.logViewer.lineRange", {
                start: pageStart + 1,
                end: pageStart + displayLines.length,
                total: matchTotal,
              })
            }}
          </p>
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            :disabled="contentLoading || pageIndex >= pageCount - 1"
            @click="changePage(1)"
          >
            {{ t("settings.logViewer.nextPage") }}
          </button>
        </div>
        <div class="flex flex-wrap gap-2">
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            :disabled="contentLoading"
            @click="openFile(selectedFile!, true)"
          >
            {{ t("settings.logViewer.refresh") }}
          </button>
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            @click="copy"
            :disabled="contentLoading || !rawContent"
          >
            {{
              copied
                ? t("settings.logViewer.copied")
                : t("settings.logViewer.copy")
            }}
          </button>
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            :disabled="contentLoading"
            @click="showOpenWithDialog"
          >
            {{ t("settings.logViewer.openWith") }}
          </button>
          <button
            type="button"
            class="ty-btn-sm ty-btn-primary-solid"
            @click="exportLogsAndReport(t)"
          >
            {{ t("settings.reportIssue.button") }}
          </button>
        </div>
      </template>
    </div>
  </div>

  <div
    v-if="props.open && openerDialogOpen"
    class="fixed inset-0 z-[60] flex items-center justify-center bg-[var(--ty-scrim)] p-4"
    @click.self="openerDialogOpen = false"
  >
    <div
      ref="openerDialogRef"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="openerTitleId"
      tabindex="-1"
      class="ty-card flex max-h-[85vh] w-full max-w-md flex-col gap-3 overflow-y-auto rounded-xl p-4 shadow-[var(--ty-overlay-shadow)] sm:p-5"
    >
      <div class="flex items-center justify-between gap-3">
        <h3 :id="openerTitleId" class="ty-section-title text-base">
          {{ t("settings.logViewer.openWithTitle") }}
        </h3>
        <button
          type="button"
          class="ty-btn-sm ty-btn-secondary"
          @click="openerDialogOpen = false"
        >
          {{ t("settings.logViewer.close") }}
        </button>
      </div>

      <div v-if="openersLoading" class="flex items-center justify-center py-6">
        <span class="loading loading-spinner loading-md opacity-60" />
      </div>

      <div
        v-else-if="openersError"
        role="alert"
        class="rounded-lg bg-[var(--ty-field)] p-3 text-sm text-[var(--ty-danger)]"
      >
        {{ openersError }}
      </div>

      <div
        v-else-if="openers.length === 0"
        class="rounded-lg bg-[var(--ty-field)] p-3 text-sm text-[var(--ty-text-muted)]"
      >
        {{ t("settings.logViewer.noOpeners") }}
      </div>

      <div v-else class="flex flex-col gap-2">
        <button
          v-for="opener in openers"
          :key="opener.id"
          type="button"
          class="log-opener-option"
          :disabled="openingEditorId !== ''"
          @click="openWith(opener.id)"
        >
          <div class="min-w-0">
            <strong class="log-opener-option__title">
              {{ displayOpenerLabel(opener) }}
            </strong>
          </div>
          <span
            v-if="openingEditorId === opener.id"
            class="loading loading-spinner loading-sm opacity-70"
          />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
:deep(.log-search-highlight) {
  border-radius: 2px;
  background: var(--ty-warning);
  color: var(--ty-canvas);
}
.log-opener-option {
  display: flex;
  width: 100%;
  align-items: center;
  justify-content: space-between;
  gap: 0.75rem;
  border-radius: 0.95rem;
  border: 1px solid var(--ty-border);
  background-color: var(--ty-field);
  padding: 0.85rem 0.95rem;
  text-align: left;
  transition:
    border-color 0.18s ease,
    background-color 0.18s ease,
    transform 0.18s ease,
    box-shadow 0.18s ease;
}

.log-opener-option:hover {
  border-color: color-mix(in srgb, var(--ty-primary) 24%, var(--ty-border));
}

.log-opener-option:disabled {
  cursor: wait;
  opacity: 0.72;
}

.log-opener-option__title {
  display: block;
  color: var(--ty-text);
  font-size: 0.84rem;
  font-weight: 700;
  line-height: 1.2;
}

.log-current-session-badge {
  background-color: color-mix(
    in srgb,
    var(--ty-primary) 12%,
    var(--ty-surface)
  );
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--ty-primary) 34%, transparent);
  color: var(--ty-primary);
  font-weight: 600;
}

.log-truncated-notice {
  background-color: color-mix(in srgb, var(--ty-warning) 8%, var(--ty-surface));
  border: 1px solid color-mix(in srgb, var(--ty-warning) 34%, var(--ty-border));
  color: var(--ty-warning);
  font-weight: 600;
}
</style>
