<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useI18n } from 'vue-i18n';
import { useFirmwareFlashContext } from '../context';
import { chipManifest } from '../chip-manifests';
import type { ErasePresetKind } from '../types';

const { t, locale } = useI18n();
const ctx = useFirmwareFlashContext();

/** TuyaOpen “device authorization” doc — locale-specific path when available. */
const tuyaopenAuthDocUrl = computed(() =>
  locale.value.toLowerCase().startsWith('zh')
    ? 'https://tuyaopen.ai/zh/docs/quick-start/equipment-authorization'
    : 'https://tuyaopen.ai/docs/quick-start/equipment-authorization'
);

/** TuyaOpen authorization code purchase page (developer platform). */
const TUYAOPEN_AUTH_PURCHASE_URL = 'https://platform.tuya.com/purchase/index?type=6';

// Only destructure actions — ref state accessed via ctx.xxx for reactivity.
const {
  onPickFile,
  onFileChange,
  applyErasePreset,
  onPickReadDir,
  onReadFileNameInput,
  startOperation,
  startAuthRead,
  addSegment,
  removeSegment,
} = ctx;

/** i18n key for each erase preset kind. */
const ERASE_PRESET_LABEL_KEYS: Record<ErasePresetKind, string> = {
  authInfo: 'flash.eraseAuthInfo',
  fullChipNoRf: 'flash.eraseFullChipNoRf',
  fullChip: 'flash.eraseFullChip',
};

/** Active erase presets for the currently selected chip, in definition order. */
const currentErasePresets = computed(() => {
  const presets = chipManifest(ctx.selectedChipId).erasePresets;
  return (Object.keys(presets) as ErasePresetKind[]).map(kind => ({
    kind,
    label: t(ERASE_PRESET_LABEL_KEYS[kind]),
  }));
});

/** Desktop table vs mobile cards — single layout in DOM to avoid duplicate v-model inputs. */
const isMdUp = ref(typeof window !== 'undefined' ? window.matchMedia('(min-width: 768px)').matches : true);

let removeMdMediaListener: (() => void) | undefined;

onMounted(() => {
  const mq = window.matchMedia('(min-width: 768px)');
  const sync = () => {
    isMdUp.value = mq.matches;
  };
  sync();
  mq.addEventListener('change', sync);
  removeMdMediaListener = () => mq.removeEventListener('change', sync);
});

onUnmounted(() => {
  removeMdMediaListener?.();
});
</script>

<template>
  <div class="flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto lg:min-w-0">
    <section class="ops-card flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      <!-- 卡片头部 -->
      <div class="ops-card-header flex items-center gap-2.5 px-3.5 py-2.5">
        <div class="ops-header-icon flex size-8 shrink-0 items-center justify-center rounded-lg" aria-hidden="true">
          <FontAwesomeIcon :icon="['fas', 'microchip']" class="size-4" />
        </div>
        <div class="min-w-0">
          <p class="ty-block-label">{{ t('flash.deviceOps') }}</p>
          <p class="mt-0.5 text-xs text-[var(--ty-text-muted)]">{{ t('flash.deviceOpsHint') }}</p>
        </div>
      </div>

      <!-- Tab 选项卡 -->
      <div class="ops-tabs mx-3 mb-3 flex gap-1 rounded-xl p-1" role="tablist" :aria-label="t('flash.tabsAria')">
        <button
          v-for="tab in ctx.tabList"
          :key="tab.id"
          type="button"
          role="tab"
          :aria-selected="ctx.activeTab === tab.id"
          class="ops-tab flex-1 rounded-lg px-3 py-1.5 text-sm font-medium transition-all duration-200"
          :class="ctx.activeTab === tab.id ? 'ops-tab-active' : 'ops-tab-inactive'"
          :disabled="ctx.busy"
          @click="ctx.activeTab = tab.id"
        >
          {{ tab.label }}
        </button>
      </div>

      <!-- 内容区 -->
      <div class="min-h-0 flex-1 overflow-y-auto px-3.5 pb-3.5">
        <div class="space-y-3">
          <!-- 烧录 Tab -->
          <div v-show="ctx.activeTab === 'flash'" class="space-y-3" role="tabpanel">
            <div class="ops-range-block rounded-xl p-3" aria-labelledby="flash-segments-title">
              <p id="flash-segments-title" class="ty-block-label mb-2.5">{{ t('flash.flashSegmentsTitle') }}</p>
              <p class="mb-3 text-xs leading-snug text-[var(--ty-text-muted)]">{{ t('flash.hexHint') }}</p>

              <!-- md+：表格布局 -->
              <div v-if="isMdUp" class="overflow-x-auto">
                <table class="w-full border-collapse text-left text-sm">
                  <thead>
                    <tr class="border-b border-[var(--ty-border)]">
                      <th scope="col" class="w-9 pb-2 pr-1 text-center align-bottom">
                        <span class="ops-field-label">#</span>
                      </th>
                      <th scope="col" class="min-w-[7.5rem] pb-2 pr-2 align-bottom">
                        <span class="ops-field-label">{{ t('flash.addrStart') }}</span>
                      </th>
                      <th scope="col" class="min-w-[7.5rem] pb-2 pr-2 align-bottom">
                        <span class="ops-field-label">{{ t('flash.addrEnd') }}</span>
                      </th>
                      <th scope="col" class="min-w-[12rem] pb-2 align-bottom">
                        <span class="ops-field-label">{{ t('flash.firmwareFile') }}</span>
                      </th>
                      <th scope="col" class="w-9 pb-2 align-bottom" aria-hidden="true" />
                    </tr>
                  </thead>
                  <tbody>
                    <tr
                      v-for="(seg, index) in ctx.flashSegments"
                      :key="seg.id"
                      class="border-b border-[var(--ty-border)] last:border-0"
                    >
                      <td
                        class="py-2.5 pr-1 text-center align-middle text-xs font-semibold text-[var(--ty-text-muted)]"
                      >
                        {{ index + 1 }}
                      </td>
                      <td class="py-2.5 pr-2 align-middle">
                        <label :for="`flash-${seg.id}-start`" class="sr-only">{{ t('flash.addrStart') }}</label>
                        <input
                          :id="`flash-${seg.id}-start`"
                          v-model="seg.startAddr"
                          type="text"
                          class="ops-text-input w-full min-w-[7rem] font-mono py-1.5 text-xs"
                          placeholder="0x00000000"
                          spellcheck="false"
                          autocomplete="off"
                          :disabled="ctx.busy"
                        />
                      </td>
                      <td class="py-2.5 pr-2 align-middle">
                        <label :for="`flash-${seg.id}-end`" class="sr-only">{{ t('flash.addrEnd') }}</label>
                        <input
                          :id="`flash-${seg.id}-end`"
                          v-model="seg.endAddr"
                          type="text"
                          class="ops-text-input w-full min-w-[7rem] font-mono py-1.5 text-xs"
                          placeholder="0x00000000"
                          spellcheck="false"
                          autocomplete="off"
                          :disabled="ctx.busy"
                        />
                      </td>
                      <td class="min-w-0 py-2.5 align-middle">
                        <div class="flex min-w-0 items-center gap-1.5">
                          <label :for="`flash-${seg.id}-file`" class="sr-only">{{ t('flash.firmwareFile') }}</label>
                          <input
                            :id="`flash-${seg.id}-file`"
                            v-model="seg.firmwarePath"
                            type="text"
                            readonly
                            :placeholder="t('flash.noFile')"
                            class="ops-text-input min-w-0 flex-1 cursor-default truncate bg-[var(--ty-surface-muted)] py-1.5 text-xs"
                          />
                          <button
                            type="button"
                            class="ops-browse-btn flex h-9 min-w-[4.75rem] shrink-0 items-center justify-center whitespace-nowrap rounded-lg px-3.5 text-sm font-semibold"
                            :disabled="ctx.busy"
                            @click="onPickFile(index)"
                          >
                            {{ t('flash.browse') }}
                          </button>
                        </div>
                      </td>
                      <td class="py-2.5 align-middle">
                        <div class="flex size-7 shrink-0 items-center justify-center">
                          <button
                            v-if="index > 0"
                            type="button"
                            class="flex size-7 items-center justify-center rounded-md text-[var(--ty-danger)] transition-colors hover:bg-[color-mix(in_srgb,var(--ty-danger)_12%,transparent)]"
                            :disabled="ctx.busy"
                            :aria-label="t('flash.removeSegment')"
                            @click="removeSegment(index)"
                          >
                            <FontAwesomeIcon :icon="['fas', 'trash']" class="size-3.5" aria-hidden="true" />
                          </button>
                        </div>
                      </td>
                    </tr>
                  </tbody>
                </table>
              </div>

              <!-- 窄屏：分段卡片 -->
              <div v-else class="space-y-3">
                <div
                  v-for="(seg, index) in ctx.flashSegments"
                  :key="seg.id"
                  class="rounded-lg border border-[var(--ty-border)] bg-[color-mix(in_srgb,var(--ty-surface-muted)_88%,transparent)] p-3"
                >
                  <p class="mb-3 text-xs font-semibold text-[var(--ty-text)]">
                    {{ t('flash.segment') }} {{ index + 1 }}
                  </p>
                  <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
                    <div>
                      <label :for="`flash-m-${seg.id}-start`" class="ops-field-label mb-1 block">{{
                        t('flash.addrStart')
                      }}</label>
                      <input
                        :id="`flash-m-${seg.id}-start`"
                        v-model="seg.startAddr"
                        type="text"
                        class="ops-text-input w-full font-mono py-1.5 text-xs"
                        placeholder="0x00000000"
                        spellcheck="false"
                        autocomplete="off"
                        :disabled="ctx.busy"
                      />
                    </div>
                    <div>
                      <label :for="`flash-m-${seg.id}-end`" class="ops-field-label mb-1 block">{{
                        t('flash.addrEnd')
                      }}</label>
                      <input
                        :id="`flash-m-${seg.id}-end`"
                        v-model="seg.endAddr"
                        type="text"
                        class="ops-text-input w-full font-mono py-1.5 text-xs"
                        placeholder="0x00000000"
                        spellcheck="false"
                        autocomplete="off"
                        :disabled="ctx.busy"
                      />
                    </div>
                  </div>
                  <div class="mt-3">
                    <label :for="`flash-m-${seg.id}-file`" class="ops-field-label mb-1 block">{{
                      t('flash.firmwareFile')
                    }}</label>
                    <div class="flex flex-col gap-2 min-[400px]:flex-row min-[400px]:items-stretch">
                      <input
                        :id="`flash-m-${seg.id}-file`"
                        v-model="seg.firmwarePath"
                        type="text"
                        readonly
                        :placeholder="t('flash.noFile')"
                        class="ops-text-input min-h-[2.25rem] min-w-0 flex-1 cursor-default truncate bg-[var(--ty-surface-muted)] py-1.5 text-xs"
                      />
                      <div class="flex shrink-0 gap-2 min-[400px]:items-stretch">
                        <button
                          type="button"
                          class="ops-browse-btn inline-flex min-h-[2.25rem] flex-1 min-[400px]:min-w-[4.75rem] min-[400px]:flex-none items-center justify-center rounded-lg px-3.5 text-sm font-semibold"
                          :disabled="ctx.busy"
                          @click="onPickFile(index)"
                        >
                          {{ t('flash.browse') }}
                        </button>
                        <button
                          v-if="index > 0"
                          type="button"
                          class="inline-flex min-h-[2.25rem] min-w-11 items-center justify-center rounded-lg border border-[color-mix(in_srgb,var(--ty-danger)_35%,transparent)] text-[var(--ty-danger)] transition-colors hover:bg-[color-mix(in_srgb,var(--ty-danger)_10%,transparent)]"
                          :disabled="ctx.busy"
                          :aria-label="t('flash.removeSegment')"
                          @click="removeSegment(index)"
                        >
                          <FontAwesomeIcon :icon="['fas', 'trash']" class="size-3.5" aria-hidden="true" />
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              </div>

              <button
                v-if="ctx.flashSegments.length < 10"
                type="button"
                class="mt-4 flex w-full items-center justify-center gap-1.5 rounded-xl border border-dashed border-[var(--ty-border)] py-2 text-[var(--ty-text-muted)] transition-all hover:border-[var(--ty-primary)] hover:text-[var(--ty-primary)]"
                :disabled="ctx.busy"
                @click="addSegment"
              >
                <FontAwesomeIcon :icon="['fas', 'plus']" class="size-3" />
                <span class="text-[11px] font-bold"
                  >{{ t('flash.addSegment') }} ({{ ctx.flashSegments.length }}/10)</span
                >
              </button>
            </div>

            <!-- Hidden file input (shared by all segments) -->
            <input
              :ref="
                el => {
                  ctx.fileInputRef = el as HTMLInputElement | null;
                }
              "
              type="file"
              class="sr-only"
              accept=".bin,.hex,.elf,.img"
              aria-hidden="true"
              tabindex="-1"
              @change="onFileChange"
            />
          </div>

          <!-- 擦除 Tab -->
          <div v-show="ctx.activeTab === 'erase'" class="space-y-3" role="tabpanel">
            <div class="ops-range-block rounded-xl p-3" aria-labelledby="erase-range-title">
              <p id="erase-range-title" class="ty-block-label mb-2.5">{{ t('flash.eraseRange') }}</p>
              <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
                <div>
                  <label for="erase-start" class="ops-field-label mb-1 block">{{ t('flash.addrStart') }}</label>
                  <input
                    id="erase-start"
                    v-model="ctx.eraseStartAddr"
                    type="text"
                    class="ops-text-input w-full font-mono py-1.5"
                    placeholder="0x00000000"
                    spellcheck="false"
                    autocomplete="off"
                    :disabled="ctx.busy"
                  />
                </div>
                <div>
                  <label for="erase-end" class="ops-field-label mb-1 block">{{ t('flash.addrEnd') }}</label>
                  <input
                    id="erase-end"
                    v-model="ctx.eraseEndAddr"
                    type="text"
                    class="ops-text-input w-full font-mono py-1.5"
                    placeholder="0x00100000"
                    spellcheck="false"
                    autocomplete="off"
                    :disabled="ctx.busy"
                  />
                </div>
              </div>
              <p class="mt-1.5 text-xs leading-snug text-[var(--ty-text-muted)]">{{ t('flash.hexHint') }}</p>
            </div>

            <div>
              <button
                type="button"
                class="ops-collapse-btn flex w-full cursor-pointer items-center justify-between rounded-lg px-2 py-1.5 text-left text-sm font-medium text-[var(--ty-text)]"
                :aria-expanded="ctx.eraseAdvancedOpen"
                aria-controls="erase-advanced-panel"
                :disabled="ctx.busy"
                @click="ctx.eraseAdvancedOpen = !ctx.eraseAdvancedOpen"
              >
                <span>{{ t('flash.eraseAdvanced') }}</span>
                <FontAwesomeIcon
                  :icon="['fas', 'chevron-down']"
                  class="size-4 shrink-0 text-[var(--ty-text-muted)] transition-transform duration-200"
                  :class="ctx.eraseAdvancedOpen ? 'rotate-180' : ''"
                  aria-hidden="true"
                />
              </button>
              <div
                v-show="ctx.eraseAdvancedOpen"
                id="erase-advanced-panel"
                class="mt-1.5 space-y-3 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface-muted)] p-3"
              >
                <p class="text-xs text-[var(--ty-text-muted)]">{{ t('flash.eraseAdvancedHint') }}</p>
                <div class="flex flex-col gap-2">
                  <button
                    v-for="preset in currentErasePresets"
                    :key="preset.kind"
                    type="button"
                    class="ty-btn-secondary min-h-9 w-full justify-center px-3 py-2 text-sm font-medium"
                    :disabled="ctx.busy"
                    @click="applyErasePreset(preset.kind)"
                  >
                    {{ preset.label }}
                  </button>
                </div>
              </div>
            </div>
          </div>

          <!-- 读取 Tab -->
          <div v-show="ctx.activeTab === 'read'" class="space-y-3" role="tabpanel">
            <!-- 保存目录 -->
            <div>
              <label for="read-dir" class="ops-field-label mb-1.5 block">{{ t('flash.readDir') }}</label>
              <div class="flex min-w-0 flex-col gap-1.5 sm:flex-row sm:items-stretch">
                <input
                  id="read-dir"
                  v-model="ctx.readDir"
                  type="text"
                  readonly
                  :placeholder="t('flash.readDirPlaceholder')"
                  class="ops-text-input min-w-0 w-full flex-1 cursor-default py-1.5"
                  aria-describedby="read-dir-hint"
                />
                <button
                  type="button"
                  class="ops-browse-btn inline-flex min-h-[2.25rem] w-full shrink-0 cursor-pointer items-center justify-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-semibold transition-colors sm:w-auto"
                  :disabled="ctx.busy"
                  @click="onPickReadDir"
                >
                  <FontAwesomeIcon :icon="['fas', 'folder-open']" class="size-3.5" aria-hidden="true" />
                  {{ t('flash.browse') }}
                </button>
              </div>
              <p id="read-dir-hint" class="mt-1 text-xs leading-snug text-[var(--ty-text-muted)]">
                {{ t('flash.readDirHint') }}
              </p>
            </div>

            <!-- 文件名称 -->
            <div>
              <label for="read-filename" class="ops-field-label mb-1.5 block">{{ t('flash.readFileName') }}</label>
              <input
                id="read-filename"
                :value="ctx.readFileName"
                type="text"
                class="ops-text-input w-full py-1.5"
                :placeholder="t('flash.readFileNamePlaceholder')"
                spellcheck="false"
                autocomplete="off"
                :disabled="ctx.busy"
                @input="onReadFileNameInput(($event.target as HTMLInputElement).value)"
              />
              <p class="mt-1 text-xs leading-snug text-[var(--ty-text-muted)]">
                {{ t('flash.readFileNameHint') }}
              </p>
            </div>

            <!-- 读取地址范围 -->
            <div class="ops-range-block rounded-xl p-3" aria-labelledby="read-range-title">
              <p id="read-range-title" class="ty-block-label mb-2.5">{{ t('flash.readRange') }}</p>
              <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
                <div>
                  <label for="read-start" class="ops-field-label mb-1 block">{{ t('flash.addrStart') }}</label>
                  <input
                    id="read-start"
                    v-model="ctx.readStartAddr"
                    type="text"
                    class="ops-text-input w-full font-mono py-1.5"
                    placeholder="0x00000000"
                    spellcheck="false"
                    autocomplete="off"
                    :disabled="ctx.busy"
                  />
                </div>
                <div>
                  <label for="read-end" class="ops-field-label mb-1 block">{{ t('flash.addrEnd') }}</label>
                  <input
                    id="read-end"
                    v-model="ctx.readEndAddr"
                    type="text"
                    class="ops-text-input w-full font-mono py-1.5"
                    placeholder="0x00200000"
                    spellcheck="false"
                    autocomplete="off"
                    :disabled="ctx.busy"
                  />
                </div>
              </div>
              <p class="mt-1.5 text-xs leading-snug text-[var(--ty-text-muted)]">{{ t('flash.hexHint') }}</p>
            </div>
          </div>

          <!-- 授权 Tab -->
          <div v-show="ctx.activeTab === 'authorize'" class="space-y-3" role="tabpanel">
            <div
              class="space-y-2 rounded-xl border border-[var(--ty-border)] bg-[var(--ty-surface-subtle)] px-3 py-2.5 text-xs leading-snug text-[var(--ty-text)]"
            >
              <p class="font-medium text-amber-800 dark:text-amber-400/95">
                <i18n-t keypath="flash.authTuyaOpenOnly" tag="span">
                  <template #link>
                    <a
                      :href="TUYAOPEN_AUTH_PURCHASE_URL"
                      target="_blank"
                      rel="noopener noreferrer"
                      class="font-semibold text-[var(--ty-accent)] underline underline-offset-2 hover:opacity-90"
                    >
                      {{ t('flash.authTuyaOpenCodeLink') }}
                    </a>
                  </template>
                </i18n-t>
              </p>
            </div>
            <div>
              <label for="auth-uuid" class="ops-field-label mb-1.5 block">{{ t('flash.uuid') }}</label>
              <input
                id="auth-uuid"
                v-model="ctx.authorizeUuid"
                type="text"
                class="ops-text-input w-full font-mono py-1.5"
                :placeholder="t('flash.uuidPh')"
                spellcheck="false"
                autocomplete="off"
                :disabled="ctx.busy"
              />
            </div>
            <div>
              <label for="auth-key" class="ops-field-label mb-1.5 block">{{ t('flash.authKey') }}</label>
              <input
                id="auth-key"
                v-model="ctx.authorizeAuthKey"
                type="text"
                class="ops-text-input w-full font-mono py-1.5"
                :placeholder="t('flash.authKeyPh')"
                spellcheck="false"
                autocomplete="off"
                :disabled="ctx.busy"
              />
            </div>
            <p class="text-xs leading-snug text-[var(--ty-text-muted)]">
              <a
                :href="tuyaopenAuthDocUrl"
                target="_blank"
                rel="noopener noreferrer"
                class="font-semibold text-[var(--ty-accent)] underline underline-offset-2 hover:opacity-90"
              >
                {{ t('flash.authOfficialDocLink') }}
              </a>
            </p>
          </div>
        </div>
      </div>

      <!-- 底部操作按钮 -->
      <div class="ops-footer shrink-0 px-3.5 pb-3.5 pt-2.5">
        <button
          v-if="ctx.activeTab === 'flash'"
          type="button"
          class="ty-btn-primary-solid w-full"
          :disabled="!ctx.canFlash"
          @click="startOperation('flash')"
        >
          <span
            v-if="ctx.busy && ctx.runningOp === 'flash'"
            class="size-4 animate-spin rounded-full border-2 border-white border-t-transparent"
            aria-hidden="true"
          />
          {{ ctx.busy && ctx.runningOp === 'flash' ? t('flash.btnFlashing') : t('flash.btnFlash') }}
        </button>
        <button
          v-else-if="ctx.activeTab === 'erase'"
          type="button"
          class="ops-btn-erase inline-flex min-h-[2.75rem] w-full cursor-pointer items-center justify-center gap-2 rounded-xl px-3 py-2.5 text-sm font-bold transition-all duration-200"
          :disabled="!ctx.canErase"
          @click="startOperation('erase')"
        >
          <span
            v-if="ctx.busy && ctx.runningOp === 'erase'"
            class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
            aria-hidden="true"
          />
          {{ ctx.busy && ctx.runningOp === 'erase' ? t('flash.btnErasing') : t('flash.btnErase') }}
        </button>
        <button
          v-else-if="ctx.activeTab === 'read'"
          type="button"
          class="ty-btn-primary-solid w-full"
          :disabled="!ctx.canRead"
          @click="startOperation('read')"
        >
          <span
            v-if="ctx.busy && ctx.runningOp === 'read'"
            class="size-4 animate-spin rounded-full border-2 border-white border-t-transparent"
            aria-hidden="true"
          />
          {{ ctx.busy && ctx.runningOp === 'read' ? t('flash.btnReading') : t('flash.btnRead') }}
        </button>
        <div v-else-if="ctx.activeTab === 'authorize'" class="flex gap-2">
          <!-- 读取授权 -->
          <button type="button" class="ty-btn-secondary flex-1" :disabled="!ctx.canReadAuth" @click="startAuthRead()">
            <span
              v-if="ctx.busy && ctx.runningOp === 'authorize' && ctx.authOpIsRead"
              class="size-4 animate-spin rounded-full border-2 border-current border-t-transparent"
              aria-hidden="true"
            />
            {{ t('flash.btnReadAuth') }}
          </button>
          <!-- 开始授权（写入） -->
          <button
            type="button"
            class="ty-btn-primary-solid flex-1"
            :disabled="!ctx.canAuthorize"
            @click="startOperation('authorize')"
          >
            <span
              v-if="ctx.busy && ctx.runningOp === 'authorize' && !ctx.authOpIsRead"
              class="size-4 animate-spin rounded-full border-2 border-white border-t-transparent"
              aria-hidden="true"
            />
            {{
              ctx.busy && ctx.runningOp === 'authorize' && !ctx.authOpIsRead
                ? t('flash.btnAuthing')
                : t('flash.btnAuth')
            }}
          </button>
        </div>
      </div>
    </section>
  </div>
</template>
