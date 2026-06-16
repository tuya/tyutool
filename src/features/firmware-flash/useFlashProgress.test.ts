// @vitest-environment happy-dom
import { describe, it, expect, vi } from "vitest";
import { useFlashProgress } from "./useFlashProgress";

function make() {
  const appendLog = vi.fn();
  const logOperationDuration = vi.fn();
  const onOperationSettled = vi.fn();
  const p = useFlashProgress({
    appendLog,
    logOperationDuration,
    onOperationSettled,
  });
  return { p, appendLog, logOperationDuration, onOperationSettled };
}

describe("useFlashProgress reducer", () => {
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
});
