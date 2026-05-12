<script setup lang="ts">
import { ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';

interface Props {
  existingNames: string[];
}

const props = defineProps<Props>();

const emit = defineEmits<{
  create: [{ name: string; useRegex: boolean }];
}>();

const { t } = useI18n();

const inputText = ref('');
const useRegex = ref(false);
const error = ref<string | null>(null);

watch(inputText, () => {
  error.value = null;
});

function tryCreate(): void {
  const name = inputText.value.trim();
  error.value = null;

  if (!name) return;

  if (props.existingNames.includes(name)) {
    error.value = t('serialDebug.subWindow.dupWarning');
    return;
  }

  if (useRegex.value) {
    try {
      new RegExp(name);
    } catch {
      error.value = t('serialDebug.subWindow.invalidRegex');
      return;
    }
  }

  emit('create', { name, useRegex: useRegex.value });
  inputText.value = '';
}
</script>

<template>
  <div class="border-b border-[var(--ty-border)] px-3 py-2">
    <div class="flex items-center gap-2">
      <input
        v-model="inputText"
        type="text"
        class="conn-select min-w-0 flex-1 text-left"
        :placeholder="t('serialDebug.subWindow.placeholder')"
        @keydown.enter="tryCreate"
      />
      <button
        type="button"
        class="shrink-0 rounded-lg px-3 py-1.5 text-xs font-semibold transition-all duration-150"
        :class="useRegex ? 'ty-btn-toggle-active' : 'conn-btn-action'"
        @click="useRegex = !useRegex"
      >
        {{ t('serialDebug.subWindow.regexLabel') }}
      </button>
      <button
        type="button"
        class="conn-btn-action shrink-0 rounded-lg px-3 py-1.5 text-xs font-semibold transition-all duration-150"
        @click="tryCreate"
      >
        {{ t('serialDebug.subWindow.addBtn') }}
      </button>
    </div>
    <p v-if="error" class="mt-1 text-xs" style="color: var(--ty-danger);">
      {{ error }}
    </p>
  </div>
</template>
