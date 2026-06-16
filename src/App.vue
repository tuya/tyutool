<script setup lang="ts">
import TyConfirmDialog from '@/components/TyConfirmDialog.vue';
import TyToast from '@/components/TyToast.vue';
import AppShell from '@/components/AppShell.vue';
import { useAutoUpdate } from '@/composables/useAutoUpdate';

useAutoUpdate();
</script>

<template>
  <AppShell />
  <TyConfirmDialog />
  <TyToast />
</template>
