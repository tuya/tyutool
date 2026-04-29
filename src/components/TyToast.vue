<script setup lang="ts">
import { watch, ref, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { useRouter } from 'vue-router';
import { toastState } from '@/composables/toastState';
import { isTauriRuntime } from '@/features/firmware-flash/flash-tauri';

const { t } = useI18n({ useScope: 'global' });
const router = useRouter();

const visible = ref(false);
let timer: ReturnType<typeof setTimeout> | null = null;

watch(
  () => toastState.visible,
  val => {
    if (val) {
      visible.value = true;
      timer = setTimeout(() => {
        dismiss();
      }, 6000);
    }
  }
);

function dismiss(): void {
  visible.value = false;
  toastState.visible = false;
  if (timer) {
    clearTimeout(timer);
    timer = null;
  }
}

function goToSettings(): void {
  dismiss();
  router.push('/settings');
  // Note: auto-opening UpdateDialog from here is complex (cross-component);
  // The toast just navigates to settings, user clicks the button there.
}

async function openPortableDownload(): Promise<void> {
  dismiss();
  const url = toastState.portableUrl || 'https://github.com/tuya/tyutool/releases/latest';
  if (isTauriRuntime()) {
    try {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(url);
      return;
    } catch {
      // fall through to window.open
    }
  }
  window.open(url, '_blank');
}

onUnmounted(() => {
  if (timer) clearTimeout(timer);
});
</script>

<template>
  <Teleport to="body">
    <Transition name="ty-toast">
      <div v-if="visible" class="ty-toast" role="alert" aria-live="polite">
        <span class="ty-toast-text">
          <template v-if="toastState.isPortable">
            {{ t('settings.update.toastPortable', { version: toastState.version }) }}
          </template>
          <template v-else>
            {{ t('settings.update.toastNew', { version: toastState.version }) }}
          </template>
        </span>
        <button v-if="toastState.isPortable" class="ty-toast-action" @click="openPortableDownload">
          {{ t('settings.update.portableDownload') }}
        </button>
        <button v-else class="ty-toast-action" @click="goToSettings">
          {{ t('settings.update.toastAction') }}
        </button>
        <button class="ty-toast-close" :aria-label="t('settings.update.toastClose')" @click="dismiss">×</button>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
.ty-toast {
  position: fixed;
  top: 1rem;
  right: 1rem;
  z-index: 9999;
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.75rem 1rem;
  background: var(--ty-surface);
  border: 1px solid var(--ty-border);
  border-radius: 0.75rem;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.12);
  max-width: 28rem;
  min-width: 16rem;
}

.ty-toast-text {
  flex: 1;
  font-size: 0.875rem;
  color: var(--ty-text);
  line-height: 1.4;
}

.ty-toast-action {
  font-size: 0.8125rem;
  font-weight: 600;
  color: var(--ty-primary);
  background: none;
  border: none;
  cursor: pointer;
  padding: 0.25rem 0.5rem;
  border-radius: 0.375rem;
  white-space: nowrap;
}

.ty-toast-action:hover {
  background: color-mix(in srgb, var(--ty-primary) 12%, transparent);
}

.ty-toast-close {
  font-size: 1rem;
  color: var(--ty-text-muted);
  background: none;
  border: none;
  cursor: pointer;
  padding: 0.125rem 0.375rem;
  border-radius: 0.375rem;
  line-height: 1;
}

.ty-toast-close:hover {
  color: var(--ty-text);
  background: var(--ty-surface-muted);
}

.ty-toast-enter-active,
.ty-toast-leave-active {
  transition:
    opacity 0.25s ease,
    transform 0.25s ease;
}

.ty-toast-enter-from,
.ty-toast-leave-to {
  opacity: 0;
  transform: translateX(1rem);
}
</style>
