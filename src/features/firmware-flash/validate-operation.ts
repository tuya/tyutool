import { i18n } from "@/i18n";
import { validateAddrRange } from "@/features/firmware-flash/hex";
import type { OpKind } from "@/features/firmware-flash/types";

const t = i18n.global.t;

function addrRangeMessage(
  err: NonNullable<ReturnType<typeof validateAddrRange>>,
): string {
  if (err === "invalid") {
    return t("flash.err.addrInvalid");
  }
  return t("flash.err.startAfterEnd");
}

export interface ValidateOperationInput {
  flashSegments: { firmwarePath: string; startAddr: string; endAddr: string }[];
  readDir: string;
  readFileName: string;
  authorizeUuid: string;
  authorizeAuthKey: string;
  selectedSerialPort: string;
  eraseStartAddr: string;
  eraseEndAddr: string;
  readStartAddr: string;
  readEndAddr: string;
  isTauri: boolean;
}

export interface ValidationFailure {
  /** User-facing message shown in the status area. */
  message: string;
  /** Line appended to the log (may differ from `message`). */
  logLine: string;
}

/** Pre-flight validation for an operation. Returns null when valid.
 *  Order of checks must match the original startOperation flow. */
export function validateOperation(
  kind: OpKind,
  input: ValidateOperationInput,
): ValidationFailure | null {
  if (kind === "flash") {
    const anyEmpty = input.flashSegments.some((s) => !s.firmwarePath.trim());
    if (anyEmpty) {
      return {
        message: t("flash.err.selectFirmware"),
        logLine: t("flash.err.selectFirmwareLog"),
      };
    }
  }
  if (kind === "read" && !input.readDir.trim() && input.isTauri) {
    return {
      message: t("flash.err.selectReadDir"),
      logLine: t("flash.err.selectReadDirLog"),
    };
  }
  if (kind === "read" && !input.readFileName.trim()) {
    return {
      message: t("flash.err.selectReadFileName"),
      logLine: t("flash.err.selectReadFileNameLog"),
    };
  }
  if (kind === "authorize") {
    const hasUuid = !!input.authorizeUuid.trim();
    const hasKey = !!input.authorizeAuthKey.trim();
    if (hasUuid !== hasKey) {
      return {
        message: t("flash.err.fillAuthLog"),
        logLine: t("flash.err.fillAuthLog"),
      };
    }
    if (hasUuid && input.authorizeUuid.trim().length !== 20) {
      const msg = t("flash.err.authUuidLen");
      return { message: msg, logLine: t("flash.err.withMsg", { msg }) };
    }
    if (hasKey && input.authorizeAuthKey.trim().length !== 32) {
      const msg = t("flash.err.authKeyLen");
      return { message: msg, logLine: t("flash.err.withMsg", { msg }) };
    }
  }
  if (!input.selectedSerialPort) {
    return {
      message: t("flash.err.deviceDisconnected"),
      logLine: t("flash.err.deviceDisconnectedLog"),
    };
  }
  if (kind === "flash") {
    for (let i = 0; i < input.flashSegments.length; i++) {
      const seg = input.flashSegments[i];
      const err = validateAddrRange(seg.startAddr, seg.endAddr);
      if (err) {
        const msg = `${t("flash.segment")} ${i + 1}: ${addrRangeMessage(err)}`;
        return { message: msg, logLine: t("flash.err.withMsg", { msg }) };
      }
    }
  }
  if (kind === "erase") {
    const err = validateAddrRange(input.eraseStartAddr, input.eraseEndAddr);
    if (err) {
      const msg = addrRangeMessage(err);
      return { message: msg, logLine: t("flash.err.withMsg", { msg }) };
    }
  }
  if (kind === "read") {
    const err = validateAddrRange(input.readStartAddr, input.readEndAddr);
    if (err) {
      const msg = addrRangeMessage(err);
      return { message: msg, logLine: t("flash.err.withMsg", { msg }) };
    }
  }
  return null;
}
