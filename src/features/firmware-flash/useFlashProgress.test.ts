// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, it, expect, vi } from "vitest";
import { i18n } from "@/i18n";

// Capture showConfirmDialog so auth_read milestone tests can assert on it
// without the real dialog singleton mutating global reactive state.
const { showConfirmDialog } = vi.hoisted(() => ({
  showConfirmDialog: vi.fn(async () => true),
}));
vi.mock("@/composables/confirmDialog", () => ({ showConfirmDialog }));

import { useFlashProgress } from "./useFlashProgress";

function make() {
  const appendLog = vi.fn();
  const logOperationDuration = vi.fn();
  const onOperationSettled = vi.fn();
  const getAuthorizeUuid = vi.fn(() => "new-uuid");
  const getAuthorizeAuthKey = vi.fn(() => "new-key");
  const sendAuthorizeConfirm = vi.fn(async (_confirmed: boolean) => {});
  const p = useFlashProgress({
    appendLog,
    logOperationDuration,
    onOperationSettled,
    getAuthorizeUuid,
    getAuthorizeAuthKey,
    sendAuthorizeConfirm,
  });
  return {
    p,
    appendLog,
    logOperationDuration,
    onOperationSettled,
    getAuthorizeUuid,
    getAuthorizeAuthKey,
    sendAuthorizeConfirm,
  };
}

describe("useFlashProgress reducer", () => {
  beforeEach(() => {
    showConfirmDialog.mockClear();
    showConfirmDialog.mockResolvedValue(true);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("clamps percent into [0,100]", () => {
    const { p } = make();
    p.handleFlashProgressPayload({ kind: "percent", value: 150 } as never);
    expect(p.flashProgress.value).toBe(100);
    p.handleFlashProgressPayload({ kind: "percent", value: -10 } as never);
    expect(p.flashProgress.value).toBe(0);
  });

  it("ignores job_summary", () => {
    const { p } = make();
    p.handleFlashProgressPayload({ kind: "job_summary" } as never);
    expect(p.flashPhase.value).toBe("idle");
  });

  it("on done/ok sets success, 100%, and runs teardown", () => {
    const { p, logOperationDuration, onOperationSettled } = make();
    p.runningOp.value = "flash";
    p.handleFlashProgressPayload({
      kind: "done",
      result: { ok: { elapsed_secs: 1.2 } },
    } as never);
    expect(p.flashPhase.value).toBe("success");
    expect(p.flashProgress.value).toBe(100);
    expect(p.runningOp.value).toBeNull();
    expect(logOperationDuration).toHaveBeenCalledOnce();
    expect(onOperationSettled).toHaveBeenCalledOnce();
  });

  it("on done/err sets error and maps the message via appendLog", () => {
    const { p, appendLog, onOperationSettled } = make();
    p.runningOp.value = "flash";
    p.handleFlashProgressPayload({
      kind: "done",
      result: { err: { message: "boom" } },
    } as never);
    expect(p.flashPhase.value).toBe("error");
    expect(appendLog).toHaveBeenCalled();
    expect(onOperationSettled).toHaveBeenCalledOnce();
  });

  it("maps serial access denied into a clearer user-facing message", () => {
    const { p, appendLog } = make();
    p.runningOp.value = "flash";
    p.handleFlashProgressPayload({
      kind: "done",
      result: { err: { message: "plugin error: serial I/O: 拒绝访问。" } },
    } as never);
    const expected = i18n.global.t("flash.err.portAccessDenied");
    expect(p.flashMessage.value).toBe(expected);
    expect(appendLog).toHaveBeenCalledWith(
      i18n.global.t("flash.err.withMsg", { msg: expected }),
    );
  });

  it("maps English serial access denied into the same clearer message", () => {
    const { p } = make();
    p.runningOp.value = "flash";
    p.handleFlashProgressPayload({
      kind: "done",
      result: {
        err: { message: "plugin error: serial I/O: Access is denied." },
      },
    } as never);
    expect(p.flashMessage.value).toBe(
      i18n.global.t("flash.err.portAccessDenied"),
    );
  });

  it("on done/cancelled sets error and still runs teardown", () => {
    const { p, onOperationSettled } = make();
    p.runningOp.value = "erase";
    p.handleFlashProgressPayload({
      kind: "done",
      result: { cancelled: {} },
    } as never);
    expect(p.flashPhase.value).toBe("error");
    expect(onOperationSettled).toHaveBeenCalledOnce();
  });

  // ── phase transitions ──────────────────────────────────────────

  it("registers a known phase (string variant) and resets phase progress", () => {
    const { p } = make();
    p.phaseProgress.value = 42;
    p.handleFlashProgressPayload({ kind: "phase", phase: "write" } as never);
    expect(p.currentBackendPhase.value).toBe("write");
    expect(p.phaseProgress.value).toBe(0);
    expect(p.phaseIndeterminate.value).toBe(false);
  });

  it("registers a struct-variant phase only when its key is in PHASE_STYLES", () => {
    const { p } = make();
    // write_segment is NOT in PHASE_STYLES → ignored, phase stays as set
    p.handleFlashProgressPayload({ kind: "phase", phase: "erase" } as never);
    expect(p.currentBackendPhase.value).toBe("erase");
    p.handleFlashProgressPayload({
      kind: "phase",
      phase: { write_segment: { current: 1, total: 3 } },
    } as never);
    // Unregistered phase ignored: still "erase"
    expect(p.currentBackendPhase.value).toBe("erase");
  });

  it("ignores an unregistered string phase (handshake) and keeps bar state", () => {
    const { p } = make();
    p.currentBackendPhase.value = "write";
    p.phaseProgress.value = 70;
    p.handleFlashProgressPayload({
      kind: "phase",
      phase: "handshake",
    } as never);
    expect(p.currentBackendPhase.value).toBe("write");
    expect(p.phaseProgress.value).toBe(70);
  });

  it("percent updates phaseProgress only while inside a registered phase", () => {
    const { p } = make();
    // No phase registered yet: phaseProgress untouched
    p.handleFlashProgressPayload({ kind: "percent", value: 30 } as never);
    expect(p.flashProgress.value).toBe(30);
    expect(p.phaseProgress.value).toBe(0);

    // Enter a registered phase, then percent should drive phaseProgress
    p.handleFlashProgressPayload({ kind: "phase", phase: "write" } as never);
    p.handleFlashProgressPayload({ kind: "percent", value: 55 } as never);
    expect(p.flashProgress.value).toBe(55);
    expect(p.phaseProgress.value).toBe(55);
    expect(p.phaseIndeterminate.value).toBe(false);
  });

  // ── indeterminate timer ─────────────────────────────────────────

  it("flips phaseIndeterminate after 2s of no percent while in a phase", () => {
    vi.useFakeTimers();
    try {
      const { p } = make();
      p.handleFlashProgressPayload({ kind: "phase", phase: "write" } as never);
      expect(p.phaseIndeterminate.value).toBe(false);
      vi.advanceTimersByTime(2000);
      expect(p.phaseIndeterminate.value).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a percent tick cancels the pending indeterminate flip", () => {
    vi.useFakeTimers();
    try {
      const { p } = make();
      p.handleFlashProgressPayload({ kind: "phase", phase: "write" } as never);
      p.handleFlashProgressPayload({ kind: "percent", value: 10 } as never);
      vi.advanceTimersByTime(2000);
      expect(p.phaseIndeterminate.value).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("cancelIndeterminateCheck stops a pending flip", () => {
    vi.useFakeTimers();
    try {
      const { p } = make();
      p.handleFlashProgressPayload({ kind: "phase", phase: "write" } as never);
      p.cancelIndeterminateCheck();
      vi.advanceTimersByTime(2000);
      expect(p.phaseIndeterminate.value).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  // ── milestones ──────────────────────────────────────────────────

  // `verify_passed` is deliberately un-translated here: it must stay a milestone
  // with no `flash.log.milestone.*` key so the fallback path keeps being tested.
  // (`handshake_complete` used to play this role until it got a real key.)
  it("string milestone with no i18n key logs a bracketed fallback", () => {
    const { p, appendLog } = make();
    p.handleFlashProgressPayload({
      kind: "milestone",
      milestone: "verify_passed",
    } as never);
    expect(appendLog).toHaveBeenCalledWith("[verify_passed]");
  });

  it("string milestone with an i18n key logs the translated text", () => {
    const { p, appendLog } = make();
    p.handleFlashProgressPayload({
      kind: "milestone",
      milestone: "handshake_complete",
    } as never);
    // Not the `[handshake_complete]` fallback — a real translation. (Locale
    // resolves from the environment, so compare against the same lookup the
    // other tests in this file use.)
    expect(appendLog).toHaveBeenCalledWith(
      i18n.global.t("flash.log.milestone.handshake_complete"),
    );
  });

  it("object milestone (connected) logs its first-key fallback", () => {
    const { p, appendLog } = make();
    p.handleFlashProgressPayload({
      kind: "milestone",
      milestone: { connected: { chip_info: "BK7231N" } },
    } as never);
    expect(appendLog).toHaveBeenCalledWith("[connected]");
  });

  it("auth_read_complete milestone shows the secure modal and logs", () => {
    const { p, appendLog } = make();
    p.handleFlashProgressPayload({
      kind: "milestone",
      milestone: {
        auth_read_complete: { uuid: "uuid-123", authkey: "key-456" },
      },
    } as never);
    expect(showConfirmDialog).toHaveBeenCalledTimes(1);
    expect(showConfirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({ showCancel: false }),
    );
    expect(appendLog).toHaveBeenCalled();
  });

  it("auth_read_empty milestone shows a warning modal and logs", () => {
    const { p, appendLog } = make();
    p.handleFlashProgressPayload({
      kind: "milestone",
      milestone: "auth_read_empty",
    } as never);
    expect(showConfirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "warning", showCancel: false }),
    );
    expect(appendLog).toHaveBeenCalled();
  });

  it("auth_conflict milestone shows confirm dialog and calls sendAuthorizeConfirm", async () => {
    showConfirmDialog.mockResolvedValueOnce(true);
    const { p, sendAuthorizeConfirm } = make();
    p.handleFlashProgressPayload({
      kind: "milestone",
      milestone: {
        auth_conflict: {
          existing_uuid: "old-uuid",
          existing_authkey: "old-key",
        },
      },
    } as never);
    // The handler is async inside a void IIFE; flush the microtask queue
    await Promise.resolve();
    await Promise.resolve();
    expect(showConfirmDialog).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "warning" }),
    );
    expect(sendAuthorizeConfirm).toHaveBeenCalledWith(true);
  });

  // ── warning ─────────────────────────────────────────────────────

  it("warning is logged with a warning glyph prefix", () => {
    const { p, appendLog } = make();
    p.handleFlashProgressPayload({
      kind: "warning",
      message: "press boot button",
    } as never);
    expect(appendLog).toHaveBeenCalledWith("⚠ press boot button");
  });

  // ── done variants for non-flash ops ─────────────────────────────

  it("done/ok sets the op-specific success message (erase, read, authorize)", () => {
    for (const op of ["erase", "read", "authorize"] as const) {
      const { p } = make();
      p.runningOp.value = op;
      p.handleFlashProgressPayload({
        kind: "done",
        result: { ok: { elapsed_secs: 0.5 } },
      } as never);
      expect(p.flashPhase.value).toBe("success");
      expect(p.flashMessage.value).not.toBe("");
    }
  });

  it("done clears phase state and resets authOpIsRead", () => {
    const { p } = make();
    p.runningOp.value = "authorize";
    p.authOpIsRead.value = true;
    p.handleFlashProgressPayload({ kind: "phase", phase: "write" } as never);
    p.handleFlashProgressPayload({
      kind: "done",
      result: { ok: { elapsed_secs: 1 } },
    } as never);
    expect(p.currentBackendPhase.value).toBeNull();
    expect(p.phaseIndeterminate.value).toBe(false);
    expect(p.authOpIsRead.value).toBe(false);
  });

  it("done/err with no message still sets error and logs", () => {
    const { p, appendLog } = make();
    p.runningOp.value = "flash";
    p.handleFlashProgressPayload({
      kind: "done",
      result: { err: { message: "" } },
    } as never);
    expect(p.flashPhase.value).toBe("error");
    expect(appendLog).toHaveBeenCalled();
  });
});
