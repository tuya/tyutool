import { inject, provide, type InjectionKey } from 'vue';
import { useFirmwareFlash, type FirmwareFlashContext } from './useFirmwareFlash';

export const firmwareFlashKey: InjectionKey<FirmwareFlashContext> = Symbol('tyutool.firmware-flash');

/**
 * Creates flash workspace state and provides it to child components (avoid prop drilling).
 */
export function provideFirmwareFlash(): FirmwareFlashContext {
  const ctx = useFirmwareFlash();
  provide(firmwareFlashKey, ctx);
  return ctx;
}

export function useFirmwareFlashContext(): FirmwareFlashContext {
  const ctx = inject(firmwareFlashKey);
  if (!ctx) {
    throw new Error('useFirmwareFlashContext() must be used under provideFirmwareFlash()');
  }
  return ctx;
}
