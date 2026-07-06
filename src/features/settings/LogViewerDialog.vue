<script setup lang="ts">
import { ref, watch, computed, nextTick } from "vue";
import { useI18n } from "vue-i18n";
import { isTauriRuntime } from "@/runtime";
import { exportLogsAndReport } from "./report-issue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

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

async function loadFileList(): Promise<void> {
  if (!isTauriRuntime()) {
    fileList.value = [];
    return;
  }
  listLoading.value = true;
  listError.value = "";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    fileList.value = await invoke<LogFileInfo[]>("list_log_files");
  } catch (e) {
    listError.value = String(e);
  } finally {
    listLoading.value = false;
  }
}

// ── Content state ──────────────────────────────────────────────────────────────

const MAX_TAIL_BYTES = 256 * 1024;

const selectedFile = ref<LogFileInfo | null>(null);
const rawContent = ref("");
const contentLoading = ref(false);
const contentError = ref("");
const isTruncated = ref(false);
const contentEl = ref<HTMLElement | null>(null);

async function openFile(file: LogFileInfo): Promise<void> {
  if (!isTauriRuntime()) return;
  selectedFile.value = file;
  searchQuery.value = "";
  currentMatchIndex.value = 0;
  rawContent.value = "";
  contentError.value = "";
  isTruncated.value = false;
  contentLoading.value = true;
  view.value = "content";
  try {
    const { invoke } = await import("@tauri-apps/api/core");
    rawContent.value = await invoke<string>("read_log_tail", {
      maxBytes: MAX_TAIL_BYTES,
      filename: file.name,
    });
    isTruncated.value = file.sizeBytes > MAX_TAIL_BYTES;
  } catch (e) {
    contentError.value = String(e);
  } finally {
    contentLoading.value = false;
    await nextTick();
    scrollToBottom();
  }
}

function backToList(): void {
  view.value = "list";
  searchQuery.value = "";
}

function scrollToBottom(): void {
  if (contentEl.value) {
    contentEl.value.scrollTop = contentEl.value.scrollHeight;
  }
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
  error: "text-red-400",
  warn: "text-yellow-400",
  info: "text-green-200",
  debug: "text-gray-500",
  trace: "text-gray-600",
  default: "text-green-200",
};

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function highlightMatch(raw: string, query: string): string {
  const escaped = escapeHtml(raw);
  if (!query) return escaped;
  const re = new RegExp(query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "gi");
  return escaped.replace(
    re,
    (m) => `<mark class="bg-yellow-300 text-black rounded-sm">${m}</mark>`,
  );
}

interface DisplayLine {
  level: LogLevel;
  html: string;
  originalIndex: number;
}

const displayLines = computed((): DisplayLine[] => {
  if (!rawContent.value) return [];
  const lines = rawContent.value.split("\n");
  const q = searchQuery.value;
  const result: DisplayLine[] = [];
  lines.forEach((raw, i) => {
    if (q && !raw.toLowerCase().includes(q.toLowerCase())) return;
    result.push({
      level: getLineLevel(raw),
      html: highlightMatch(raw, q),
      originalIndex: i,
    });
  });
  return result;
});

const matchTotal = computed(() => displayLines.value.length);

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
  nextTick(() => {
    const el =
      contentEl.value?.querySelectorAll<HTMLElement>("[data-line]")[idx];
    el?.scrollIntoView({ block: "center" });
  });
}

watch(searchQuery, () => {
  currentMatchIndex.value = 0;
});

// ── Copy ───────────────────────────────────────────────────────────────────────

const copied = ref(false);

async function copy(): Promise<void> {
  try {
    await navigator.clipboard.writeText(rawContent.value);
    copied.value = true;
    setTimeout(() => (copied.value = false), 1500);
  } catch {
    /* ignore */
  }
}

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
    if (open) {
      view.value = "list";
      void loadFileList();
    }
  },
);
</script>

<template>
  <div
    v-if="props.open"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
    @click.self="emit('close')"
  >
    <div
      class="ty-card flex max-h-[85vh] w-full max-w-3xl flex-col gap-3 rounded-xl p-4 sm:p-5"
    >
      <!-- ── Header ── -->
      <div class="flex items-center justify-between">
        <h2 class="ty-section-title">{{ t("settings.logViewer.title") }}</h2>
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
          class="flex flex-1 items-center justify-center py-8"
        >
          <span class="loading loading-spinner loading-md opacity-60" />
        </div>

        <!-- Error -->
        <div
          v-else-if="listError"
          class="rounded-lg bg-red-950/40 p-3 text-sm text-red-400"
        >
          {{ listError }}
        </div>

        <!-- Empty -->
        <div
          v-else-if="fileList.length === 0"
          class="flex flex-1 items-center justify-center py-8 text-sm opacity-50"
        >
          {{ t("settings.logViewer.empty") }}
        </div>

        <!-- Table -->
        <div v-else class="min-h-0 flex-1 overflow-auto">
          <table class="w-full text-sm">
            <thead>
              <tr class="border-b border-white/10 text-left text-xs opacity-60">
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
                class="cursor-pointer border-b border-white/5 hover:bg-white/5"
                @click="openFile(file)"
              >
                <td class="py-1.5 pr-4 font-mono text-xs">
                  {{ fileStem(file.name) }}
                  <span
                    v-if="idx === 0"
                    class="log-current-session-badge ml-1.5 rounded px-1 py-0.5 text-[10px]"
                  >
                    {{ t("settings.logViewer.currentSession") }}
                  </span>
                </td>
                <td class="py-1.5 pr-4 tabular-nums opacity-60">
                  {{ formatSize(file.sizeBytes) }}
                </td>
                <td class="py-1.5 tabular-nums opacity-60">
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
        <div class="flex items-center gap-1 text-sm">
          <button
            type="button"
            class="text-blue-400 hover:underline"
            @click="backToList"
          >
            {{ t("settings.logViewer.fileList") }}
          </button>
          <span class="opacity-40">/</span>
          <span class="font-mono text-xs opacity-70">{{
            fileStem(selectedFile?.name ?? "")
          }}</span>
        </div>

        <!-- Truncation notice -->
        <div
          v-if="isTruncated"
          class="log-truncated-notice rounded px-3 py-1.5 text-xs"
        >
          {{ t("settings.logViewer.truncated") }}
        </div>

        <!-- Search bar -->
        <div class="flex items-center gap-2">
          <input
            v-model="searchQuery"
            type="text"
            class="ty-input flex-1 font-mono text-xs"
            :placeholder="t('settings.logViewer.searchPlaceholder')"
          />
          <span v-if="searchQuery" class="shrink-0 text-xs opacity-60">
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
          >
            ↑
          </button>
          <button
            v-if="searchQuery"
            type="button"
            class="ty-btn-sm ty-btn-secondary px-2"
            :disabled="matchTotal === 0"
            @click="jumpNext"
          >
            ↓
          </button>
        </div>

        <!-- Log content -->
        <div
          v-if="contentLoading"
          class="flex flex-1 items-center justify-center py-8"
        >
          <span class="loading loading-spinner loading-md opacity-60" />
        </div>

        <div
          v-else-if="contentError"
          class="min-h-0 flex-1 overflow-auto rounded-lg bg-black/80 p-3 font-mono text-xs text-red-400"
        >
          {{ contentError }}
        </div>

        <div
          v-else
          ref="contentEl"
          class="min-h-0 flex-1 overflow-auto rounded-lg bg-black/80 p-3 font-mono text-xs leading-relaxed"
        >
          <div v-if="displayLines.length === 0" class="opacity-40">
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
            :class="[
              LEVEL_CLASS[line.level],
              idx === currentMatchIndex && searchQuery
                ? 'bg-white/10 rounded'
                : '',
            ]"
            v-html="line.html"
          />
        </div>

        <!-- Footer -->
        <div class="flex flex-wrap gap-2">
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            :disabled="contentLoading"
            @click="openFile(selectedFile!)"
          >
            {{ t("settings.logViewer.refresh") }}
          </button>
          <button
            type="button"
            class="ty-btn-sm ty-btn-secondary"
            @click="copy"
          >
            {{
              copied
                ? t("settings.logViewer.copied")
                : t("settings.logViewer.copy")
            }}
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
</template>

<style scoped>
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
  background-color: color-mix(in srgb, var(--ty-accent) 10%, var(--ty-surface));
  border: 1px solid color-mix(in srgb, var(--ty-accent) 34%, var(--ty-border));
  color: var(--ty-accent-hover);
  font-weight: 600;
}
</style>
