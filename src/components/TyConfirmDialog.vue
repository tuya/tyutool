<script setup lang="ts">
import { watch, ref, onMounted, onUnmounted, computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { confirmDialogState, resolveConfirmDialog } from '@/composables/confirmDialog';

const { t } = useI18n({ useScope: 'global' });

const dialogRef = ref<HTMLDivElement | null>(null);
const okBtnRef = ref<HTMLButtonElement | null>(null);
const extraActionBusy = ref(false);

function handleConfirm(): void {
  resolveConfirmDialog(true);
}

function handleCancel(): void {
  resolveConfirmDialog(false);
}

async function handleExtraAction(): Promise<void> {
  if (!confirmDialogState.onExtraAction || extraActionBusy.value) {
    return;
  }
  try {
    extraActionBusy.value = true;
    await confirmDialogState.onExtraAction();
  } finally {
    extraActionBusy.value = false;
  }
}

function onKeydown(e: KeyboardEvent): void {
  if (!confirmDialogState.visible) return;
  if (e.key === 'Escape') {
    e.preventDefault();
    handleCancel();
  }
}

// Focus the confirm button when dialog opens for keyboard accessibility.
watch(
  () => confirmDialogState.visible,
  visible => {
    if (visible) {
      requestAnimationFrame(() => {
        okBtnRef.value?.focus();
      });
    } else {
      extraActionBusy.value = false;
    }
  }
);

onMounted(() => {
  document.addEventListener('keydown', onKeydown);
});

onUnmounted(() => {
  document.removeEventListener('keydown', onKeydown);
});

const kindIcon: Record<string, [string, string]> = {
  info: ['fas', 'circle-info'],
  warning: ['fas', 'triangle-exclamation'],
  danger: ['fas', 'triangle-exclamation'],
};

/** Hover / assistive hint: what the header icon represents (dialog severity). */
const dialogIconPurpose = computed(() => {
  const k = confirmDialogState.kind;
  if (k === 'warning') {
    return t('common.dialogIconPurposeWarning');
  }
  if (k === 'danger') {
    return t('common.dialogIconPurposeDanger');
  }
  return t('common.dialogIconPurposeInfo');
});

/** Short visible label next to the icon (what the icon means at a glance). */
const dialogKindBadge = computed(() => {
  const k = confirmDialogState.kind;
  if (k === 'warning') {
    return t('common.dialogKindBadgeWarning');
  }
  if (k === 'danger') {
    return t('common.dialogKindBadgeDanger');
  }
  return t('common.dialogKindBadgeInfo');
});
</script>

<template>
  <Teleport to="body">
    <Transition name="ty-dialog">
      <div v-if="confirmDialogState.visible" class="ty-dialog-backdrop" role="presentation" @click.self="handleCancel">
        <div
          ref="dialogRef"
          class="ty-dialog-container"
          role="alertdialog"
          aria-modal="true"
          :aria-labelledby="'ty-dialog-title'"
          :aria-describedby="'ty-dialog-message'"
        >
          <!-- ── Top accent bar ── -->
          <div
            class="ty-dialog-accent-bar"
            :class="{
              'ty-dialog-accent-warning': confirmDialogState.kind === 'warning',
              'ty-dialog-accent-danger': confirmDialogState.kind === 'danger',
              'ty-dialog-accent-info': confirmDialogState.kind === 'info',
            }"
            aria-hidden="true"
          />

          <!-- ── Header: kind icon + title + close ── -->
          <div class="ty-dialog-header">
            <div class="ty-dialog-header-main">
              <div
                class="ty-dialog-icon-circle"
                :class="{
                  'ty-dialog-icon-warning': confirmDialogState.kind === 'warning',
                  'ty-dialog-icon-danger': confirmDialogState.kind === 'danger',
                  'ty-dialog-icon-info': confirmDialogState.kind === 'info',
                }"
                role="img"
                :aria-label="dialogIconPurpose"
                :title="dialogIconPurpose"
              >
                <FontAwesomeIcon
                  :icon="kindIcon[confirmDialogState.kind] ?? kindIcon.info"
                  class="size-[1.125rem]"
                  aria-hidden="true"
                />
              </div>
              <span class="ty-dialog-kind-badge" aria-hidden="true">{{ dialogKindBadge }}</span>
              <h2 id="ty-dialog-title" class="ty-dialog-title">
                {{ confirmDialogState.title }}
              </h2>
            </div>
            <button type="button" class="ty-dialog-close" :aria-label="t('common.closeDialog')" @click="handleCancel">
              <FontAwesomeIcon :icon="['fas', 'xmark']" class="size-4" aria-hidden="true" />
            </button>
          </div>

          <!-- ── Body ── -->
          <div id="ty-dialog-message" class="ty-dialog-body">
            <p
              v-for="(line, i) in confirmDialogState.message.split('\n')"
              :key="i"
              :class="line.trim() === '' ? 'ty-dialog-spacer' : ''"
            >
              {{ line }}
            </p>
          </div>

          <!-- ── Footer ── -->
          <div
            class="ty-dialog-footer"
            :class="{
              'ty-dialog-footer--single': !confirmDialogState.showCancel && !confirmDialogState.extraActionLabel,
            }"
          >
            <button
              v-if="confirmDialogState.showCancel"
              type="button"
              class="ty-dialog-btn ty-dialog-btn-cancel"
              @click="handleCancel"
            >
              {{ confirmDialogState.cancelLabel }}
            </button>
            <button
              v-if="confirmDialogState.extraActionLabel"
              type="button"
              class="ty-dialog-btn ty-dialog-btn-cancel"
              :disabled="extraActionBusy"
              @click="handleExtraAction"
            >
              {{ confirmDialogState.extraActionLabel }}
            </button>
            <button
              ref="okBtnRef"
              type="button"
              class="ty-dialog-btn ty-dialog-btn-ok"
              :class="{
                'ty-dialog-btn-ok-danger':
                  confirmDialogState.kind === 'danger' || confirmDialogState.kind === 'warning',
                'ty-dialog-btn-ok-primary': confirmDialogState.kind === 'info',
              }"
              @click="handleConfirm"
            >
              {{ confirmDialogState.okLabel }}
            </button>
          </div>
        </div>
      </div>
    </Transition>
  </Teleport>
</template>

<style scoped>
/* ── Backdrop ── */
.ty-dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 9999;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  padding: 1.5rem;
}

/* ── Container ── */
.ty-dialog-container {
  position: relative;
  width: 100%;
  max-width: 25rem;
  border-radius: 1rem;
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface);
  box-shadow:
    0 12px 40px rgba(0, 0, 0, 0.2),
    0 4px 12px rgba(0, 0, 0, 0.1),
    inset 0 1px 0 rgba(255, 255, 255, 0.06);
  overflow: hidden;
}

.dark .ty-dialog-container {
  box-shadow:
    0 12px 40px rgba(0, 0, 0, 0.6),
    0 4px 12px rgba(0, 0, 0, 0.35),
    inset 0 1px 0 rgba(255, 255, 255, 0.04);
}

/* ── Top accent bar (3px, same pattern as aside-log-card border-top) ── */
.ty-dialog-accent-bar {
  height: 3px;
  width: 100%;
}

.ty-dialog-accent-warning {
  background: linear-gradient(90deg, var(--ty-accent), color-mix(in srgb, var(--ty-accent) 60%, transparent));
}

.ty-dialog-accent-danger {
  background: linear-gradient(90deg, var(--ty-danger), color-mix(in srgb, var(--ty-danger) 60%, transparent));
}

.ty-dialog-accent-info {
  background: linear-gradient(90deg, var(--ty-primary), color-mix(in srgb, var(--ty-primary) 60%, transparent));
}

/* ── Header (icon + title + close) ── */
.ty-dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 0.5rem;
  padding: 1rem 0.75rem 0 1.25rem;
}

.ty-dialog-header-main {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  min-width: 0;
  flex: 1;
}

.ty-dialog-kind-badge {
  flex-shrink: 0;
  align-self: center;
  font-size: 0.6875rem;
  font-weight: 600;
  line-height: 1.2;
  padding: 0.2rem 0.45rem;
  border-radius: 0.25rem;
  color: var(--ty-text-muted);
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface-muted);
}

.ty-dialog-icon-circle {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 0.625rem;
  flex-shrink: 0;
}

.ty-dialog-close {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 1.875rem;
  height: 1.875rem;
  margin: 0;
  flex-shrink: 0;
  border-radius: 0.5rem;
  border: 1px solid var(--ty-border);
  background: transparent;
  color: var(--ty-text-muted);
  cursor: pointer;
  transition:
    background-color 0.15s ease,
    color 0.15s ease,
    border-color 0.15s ease,
    box-shadow 0.15s ease;
}

.ty-dialog-close:hover {
  background-color: color-mix(in srgb, var(--ty-danger) 12%, var(--ty-surface-muted));
  border-color: color-mix(in srgb, var(--ty-danger) 40%, var(--ty-border));
  color: var(--ty-danger);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.12);
}

.ty-dialog-close:active {
  transform: scale(0.95);
}

.ty-dialog-icon-warning {
  background: color-mix(in srgb, var(--ty-accent) 14%, transparent);
  color: var(--ty-accent);
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--ty-accent) 22%, transparent);
}

.ty-dialog-icon-danger {
  background: color-mix(in srgb, var(--ty-danger) 12%, transparent);
  color: var(--ty-danger);
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--ty-danger) 22%, transparent);
}

.ty-dialog-icon-info {
  background: color-mix(in srgb, var(--ty-primary) 12%, transparent);
  color: var(--ty-primary);
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--ty-primary) 22%, transparent);
}

/* ── Title ── */
.ty-dialog-title {
  text-align: left;
  font-size: 0.9375rem;
  font-weight: 600;
  color: var(--ty-text);
  line-height: 1.35;
  padding: 0;
  margin: 0;
  letter-spacing: -0.01em;
  flex: 1;
  min-width: 0;
}

/* ── Body ── */
.ty-dialog-body {
  padding: 0.625rem 1.5rem 0.25rem;
  text-align: left;
  font-size: 0.8125rem;
  line-height: 1.65;
  color: var(--ty-text-muted);
}

.ty-dialog-spacer {
  height: 0.5rem;
}

/* ── Footer ── */
.ty-dialog-footer {
  display: flex;
  gap: 0.625rem;
  padding: 1.25rem 1.5rem 1.5rem;
}

.ty-dialog-footer--single {
  justify-content: stretch;
}

.ty-dialog-footer--single .ty-dialog-btn-ok {
  flex: 1;
}

/* ── Buttons (shared) ── */
.ty-dialog-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: 1;
  min-height: 2.375rem;
  border-radius: 0.625rem;
  padding: 0.5rem 1rem;
  font-size: 0.8125rem;
  font-weight: 600;
  cursor: pointer;
  transition:
    background-color 0.18s ease,
    border-color 0.18s ease,
    filter 0.18s ease,
    transform 0.12s ease;
}

.ty-dialog-btn:active {
  transform: scale(0.98);
}

/* Cancel button */
.ty-dialog-btn-cancel {
  border: 1px solid var(--ty-border);
  background-color: var(--ty-surface);
  color: var(--ty-text);
}

.ty-dialog-btn-cancel:hover {
  background-color: var(--ty-surface-muted);
  border-color: var(--ty-border-strong);
}

/* OK button */
.ty-dialog-btn-ok {
  border: none;
  color: #fff;
}

.ty-dialog-btn-ok:hover {
  filter: brightness(1.06);
}

.ty-dialog-btn-ok-danger {
  background: linear-gradient(135deg, var(--ty-danger) 0%, color-mix(in srgb, var(--ty-danger) 82%, #000) 100%);
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.12),
    0 4px 14px color-mix(in srgb, var(--ty-danger) 28%, transparent);
}

.ty-dialog-btn-ok-primary {
  background: linear-gradient(135deg, var(--ty-primary) 0%, var(--ty-primary-hover) 100%);
  box-shadow:
    0 1px 3px rgba(0, 0, 0, 0.12),
    0 4px 14px color-mix(in srgb, var(--ty-primary) 28%, transparent);
}

/* ── Transition ── */
.ty-dialog-enter-active {
  transition: opacity 0.22s ease-out;
}

.ty-dialog-leave-active {
  transition: opacity 0.15s ease-in;
}

.ty-dialog-enter-from,
.ty-dialog-leave-to {
  opacity: 0;
}

.ty-dialog-enter-active .ty-dialog-container {
  animation: ty-dialog-in 0.22s cubic-bezier(0.34, 1.56, 0.64, 1);
}

.ty-dialog-leave-active .ty-dialog-container {
  animation: ty-dialog-out 0.15s ease-in forwards;
}

@keyframes ty-dialog-in {
  from {
    opacity: 0;
    transform: scale(0.92) translateY(8px);
  }
  to {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
}

@keyframes ty-dialog-out {
  from {
    opacity: 1;
    transform: scale(1) translateY(0);
  }
  to {
    opacity: 0;
    transform: scale(0.95) translateY(4px);
  }
}

/* ── Reduced motion ── */
@media (prefers-reduced-motion: reduce) {
  .ty-dialog-enter-active,
  .ty-dialog-leave-active {
    transition-duration: 0.01ms !important;
  }

  .ty-dialog-enter-active .ty-dialog-container,
  .ty-dialog-leave-active .ty-dialog-container {
    animation-duration: 0.01ms !important;
  }
}
</style>
