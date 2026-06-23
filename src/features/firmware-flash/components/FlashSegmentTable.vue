<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { useFirmwareFlashContext } from "../context";

/**
 * Editor for the flash address segments.
 *
 * Renders a desktop table and a mobile card list side-by-side, switching
 * visibility purely via Tailwind `md:` classes — no JS media query, so the
 * focus on an active <input> survives a window resize across 768px.
 *
 * The two layouts have non-colliding input ids (`flash-` vs `flash-m-`),
 * so both can live in the DOM simultaneously.
 */

const { t } = useI18n();
const ctx = useFirmwareFlashContext();
const { onPickFile, addSegment, removeSegment } = ctx;

const MAX_SEGMENTS = 10;
</script>

<template>
  <div
    class="ops-range-block rounded-xl p-3"
    aria-labelledby="flash-segments-title"
  >
    <p id="flash-segments-title" class="ty-block-label mb-2.5">
      {{ t("flash.flashSegmentsTitle") }}
    </p>
    <p class="mb-3 text-xs leading-snug text-[var(--ty-text-muted)]">
      {{ t("flash.hexHint") }}
    </p>

    <!-- md+: table layout -->
    <div class="hidden overflow-x-auto md:block">
      <table class="w-full border-collapse text-left text-sm">
        <thead>
          <tr class="border-b border-[var(--ty-border)]">
            <th scope="col" class="w-9 pb-2 pr-1 text-center align-bottom">
              <span class="ops-field-label">#</span>
            </th>
            <th scope="col" class="min-w-[7.5rem] pb-2 pr-2 align-bottom">
              <span class="ops-field-label">{{ t("flash.addrStart") }}</span>
            </th>
            <th scope="col" class="min-w-[7.5rem] pb-2 pr-2 align-bottom">
              <span class="ops-field-label">{{ t("flash.addrEnd") }}</span>
            </th>
            <th scope="col" class="min-w-[12rem] pb-2 align-bottom">
              <span class="ops-field-label">{{ t("flash.firmwareFile") }}</span>
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
              <label :for="`flash-${seg.id}-start`" class="sr-only">{{
                t("flash.addrStart")
              }}</label>
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
              <label :for="`flash-${seg.id}-end`" class="sr-only">{{
                t("flash.addrEnd")
              }}</label>
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
                <label :for="`flash-${seg.id}-file`" class="sr-only">{{
                  t("flash.firmwareFile")
                }}</label>
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
                  {{ t("flash.browse") }}
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
                  <FontAwesomeIcon
                    :icon="['fas', 'trash']"
                    class="size-3.5"
                    aria-hidden="true"
                  />
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
    </div>

    <!-- Narrow screen: card layout -->
    <div class="space-y-3 md:hidden">
      <div
        v-for="(seg, index) in ctx.flashSegments"
        :key="seg.id"
        class="rounded-lg border border-[var(--ty-border)] bg-[color-mix(in_srgb,var(--ty-surface-muted)_88%,transparent)] p-3"
      >
        <p class="mb-3 text-xs font-semibold text-[var(--ty-text)]">
          {{ t("flash.segment") }} {{ index + 1 }}
        </p>
        <div class="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <div>
            <label
              :for="`flash-m-${seg.id}-start`"
              class="ops-field-label mb-1 block"
              >{{ t("flash.addrStart") }}</label
            >
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
            <label
              :for="`flash-m-${seg.id}-end`"
              class="ops-field-label mb-1 block"
              >{{ t("flash.addrEnd") }}</label
            >
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
          <label
            :for="`flash-m-${seg.id}-file`"
            class="ops-field-label mb-1 block"
            >{{ t("flash.firmwareFile") }}</label
          >
          <div
            class="flex flex-col gap-2 min-[400px]:flex-row min-[400px]:items-stretch"
          >
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
                {{ t("flash.browse") }}
              </button>
              <button
                v-if="index > 0"
                type="button"
                class="inline-flex min-h-[2.25rem] min-w-11 items-center justify-center rounded-lg border border-[color-mix(in_srgb,var(--ty-danger)_35%,transparent)] text-[var(--ty-danger)] transition-colors hover:bg-[color-mix(in_srgb,var(--ty-danger)_10%,transparent)]"
                :disabled="ctx.busy"
                :aria-label="t('flash.removeSegment')"
                @click="removeSegment(index)"
              >
                <FontAwesomeIcon
                  :icon="['fas', 'trash']"
                  class="size-3.5"
                  aria-hidden="true"
                />
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>

    <button
      v-if="ctx.flashSegments.length < MAX_SEGMENTS"
      type="button"
      class="mt-4 flex w-full items-center justify-center gap-1.5 rounded-xl border border-dashed border-[var(--ty-border)] py-2 text-[var(--ty-text-muted)] transition-all hover:border-[var(--ty-primary)] hover:text-[var(--ty-primary)]"
      :disabled="ctx.busy"
      @click="addSegment"
    >
      <FontAwesomeIcon :icon="['fas', 'plus']" class="size-3" />
      <span class="text-[11px] font-bold"
        >{{ t("flash.addSegment") }} ({{ ctx.flashSegments.length }}/{{
          MAX_SEGMENTS
        }})</span
      >
    </button>
  </div>
</template>
