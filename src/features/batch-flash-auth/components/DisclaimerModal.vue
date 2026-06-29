<!-- src/features/batch-flash-auth/components/DisclaimerModal.vue -->
<script setup lang="ts">
import { ref, watch } from "vue";
import { useI18n } from "vue-i18n";

const { t } = useI18n();

const props = defineProps<{ show: boolean }>();
const emit = defineEmits<{
  confirm: [dontShowAgain: boolean];
  cancel: [];
}>();

const dontShowAgain = ref(false);
const dialogRef = ref<HTMLDialogElement | null>(null);

watch(
  () => props.show,
  (val) => {
    if (val) {
      dontShowAgain.value = false;
      dialogRef.value?.showModal();
    } else {
      dialogRef.value?.close();
    }
  },
);
</script>

<template>
  <Teleport to="body">
    <dialog
      ref="dialogRef"
      class="m-auto max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-[var(--ty-border)] bg-[var(--ty-surface)] p-0 shadow-xl backdrop:bg-black/50"
      @cancel.prevent
    >
      <div class="p-5">
        <!-- Title -->
        <h2 class="mb-4 text-base font-semibold text-[var(--ty-text)]">
          {{ t("batchFlashAuth.disclaimer.title") }}
        </h2>

        <!-- Notice items -->
        <ol class="mb-5 flex flex-col gap-3">
          <li class="rounded-lg bg-[var(--ty-surface-muted)] p-3">
            <p class="mb-1 text-sm font-semibold text-[var(--ty-text)]">
              {{ t("batchFlashAuth.disclaimer.item1.title") }}
            </p>
            <p class="text-xs leading-relaxed text-[var(--ty-text-muted)]">
              {{ t("batchFlashAuth.disclaimer.item1.body") }}
            </p>
          </li>
          <li class="rounded-lg bg-[var(--ty-surface-muted)] p-3">
            <p class="mb-1 text-sm font-semibold text-[var(--ty-text)]">
              {{ t("batchFlashAuth.disclaimer.item2.title") }}
            </p>
            <p class="text-xs leading-relaxed text-[var(--ty-text-muted)]">
              {{ t("batchFlashAuth.disclaimer.item2.body") }}
            </p>
          </li>
          <li class="rounded-lg bg-[var(--ty-surface-muted)] p-3">
            <p class="mb-1 text-sm font-semibold text-[var(--ty-text)]">
              {{ t("batchFlashAuth.disclaimer.item3.title") }}
            </p>
            <p class="text-xs leading-relaxed text-[var(--ty-text-muted)]">
              {{ t("batchFlashAuth.disclaimer.item3.body") }}
            </p>
          </li>
          <li class="rounded-lg bg-[var(--ty-surface-muted)] p-3">
            <p class="mb-1 text-sm font-semibold text-[var(--ty-text)]">
              {{ t("batchFlashAuth.disclaimer.item4.title") }}
            </p>
            <p class="text-xs leading-relaxed text-[var(--ty-text-muted)]">
              {{ t("batchFlashAuth.disclaimer.item4.body") }}
            </p>
          </li>
          <li class="rounded-lg bg-[var(--ty-surface-muted)] p-3">
            <p class="mb-1 text-sm font-semibold text-[var(--ty-text)]">
              {{ t("batchFlashAuth.disclaimer.item5.title") }}
            </p>
            <p class="text-xs leading-relaxed text-[var(--ty-text-muted)]">
              {{ t("batchFlashAuth.disclaimer.item5.body") }}
            </p>
          </li>
        </ol>

        <!-- Don't show again checkbox -->
        <label
          class="mb-4 flex cursor-pointer items-center gap-2 text-sm text-[var(--ty-text-muted)]"
        >
          <input
            v-model="dontShowAgain"
            type="checkbox"
            class="size-4 accent-[var(--ty-primary)]"
          />
          {{ t("batchFlashAuth.disclaimer.dontShowAgain") }}
        </label>

        <!-- Buttons -->
        <div class="flex flex-col gap-2 sm:flex-row-reverse">
          <button
            type="button"
            class="ty-btn-primary-solid flex-1"
            @click="emit('confirm', dontShowAgain)"
          >
            {{ t("batchFlashAuth.disclaimer.confirm") }}
          </button>
          <button
            type="button"
            class="ty-btn-secondary flex-1"
            @click="emit('cancel')"
          >
            {{ t("batchFlashAuth.disclaimer.cancel") }}
          </button>
        </div>
      </div>
    </dialog>
  </Teleport>
</template>
