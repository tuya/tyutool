<!-- src/features/toolbox/ToolboxPage.vue -->
<script setup lang="ts">
import TyPageHeader from "@/components/TyPageHeader.vue";
import { computed, unref } from "vue";
import { useI18n } from "vue-i18n";
import TyPortActivityIndicator from "@/components/TyPortActivityIndicator.vue";
import TyBatchFlashAuthIcon from "@/components/icons/TyBatchFlashAuthIcon.vue";
import { useFeaturePortIndicators } from "@/features/serial-port-indicators/useFeaturePortIndicators";
import { TOOLBOX_TOOLS } from "./tools";

const { t } = useI18n();
const portIndicators = useFeaturePortIndicators();
const indicatorPaletteMode = computed(() => unref(portIndicators.paletteMode));
const indicatorActivePorts = computed(() => unref(portIndicators.activePorts));

const tools = computed(() =>
  TOOLBOX_TOOLS.map((tool) => ({
    ...tool,
    name: t(tool.nameKey),
    desc: t(tool.descKey),
  })),
);
</script>

<template>
  <div class="flex min-w-0 flex-col gap-3 sm:gap-4">
    <TyPageHeader
      :title="t('app.nav.toolbox')"
      :section="t('toolbox.section')"
      :description="t('toolbox.subtitle')"
    />

    <div
      class="grid min-w-0 gap-3 sm:gap-4"
      :class="
        tools.length >= 2 ? 'sm:grid-cols-2 xl:grid-cols-3' : 'sm:grid-cols-1'
      "
    >
      <RouterLink
        v-for="tool in tools"
        :key="tool.to"
        :to="tool.to"
        :data-tool-id="tool.id"
        class="ty-card tool-card group flex cursor-pointer items-center gap-6 p-6 transition-colors hover:bg-[var(--ty-field)]"
      >
        <div
          class="tool-card-icon flex size-14 shrink-0 items-center justify-center text-[var(--ty-primary)]"
        >
          <TyBatchFlashAuthIcon
            v-if="tool.icon === 'batch-flash-auth'"
            class="size-8 text-[var(--ty-primary)]"
          />
        </div>
        <div class="min-w-0 flex-1">
          <div class="flex min-w-0 items-center gap-2">
            <p
              class="min-w-0 flex-1 text-lg font-semibold text-[var(--ty-text)]"
            >
              {{ tool.name }}
            </p>
            <TyPortActivityIndicator
              v-if="tool.id === 'batch-flash-auth'"
              :indicator="portIndicators.indicatorForFeature('toolbox')"
              :active-ports="indicatorActivePorts"
              :palette-mode="indicatorPaletteMode"
              feature="toolbox"
              surface="toolbox-card"
            />
          </div>
          <p
            class="mt-2 max-w-xl text-[13px] leading-relaxed text-[var(--ty-text-muted)]"
          >
            {{ tool.desc }}
          </p>
          <span
            class="mt-5 inline-flex items-center gap-2 rounded-md bg-[var(--ty-primary-soft)] px-3 py-2 text-xs font-semibold text-[var(--ty-primary)]"
            >{{ t("toolbox.openTool")
            }}<FontAwesomeIcon
              :icon="['fas', 'arrow-right']"
              aria-hidden="true"
          /></span>
        </div>
        <FontAwesomeIcon
          :icon="['fas', 'chevron-right']"
          class="size-3.5 shrink-0 text-[var(--ty-text-muted)] transition-transform group-hover:translate-x-0.5"
          aria-hidden="true"
        />
      </RouterLink>
    </div>
  </div>
</template>

<style scoped>
.tool-card {
  min-height: 220px;
}
@media (max-width: 600px) {
  .tool-card {
    min-height: 180px;
    gap: 16px;
    padding: 20px;
  }
  .tool-card-icon {
    width: 32px;
  }
}
</style>
