import { reactive } from 'vue';

export const toastState = reactive({
  visible: false,
  version: '',
  isPortable: false,
  portableUrl: '',
});
