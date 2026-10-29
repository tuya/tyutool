import { describe, expect, it, vi } from "vitest";
import { HISTORY_PAGE_SIZE } from "./constants";
import { useSerialDebugHistory } from "./useSerialDebugHistory";
import type { SerialDebugLine, SerialDebugSessionPage } from "./types";

vi.mock("@/i18n", () => ({
  i18n: { global: { t: (key: string) => key } },
}));

function lines(start: number, count: number): SerialDebugLine[] {
  return Array.from({ length: count }, (_, i) => ({
    lineNo: start + i + 1,
    tsMs: 1000 + start + i,
    direction: "rx",
    text: `line-${start + i + 1}`,
  }));
}

function page(
  start: number,
  limit: number,
  totalLines: number,
): SerialDebugSessionPage {
  const actualStart = Math.min(start, totalLines);
  const items = lines(actualStart, Math.min(limit, totalLines - actualStart));
  return { start: actualStart, totalLines, items };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function history(
  readSessionPage: (
    start: number,
    limit: number,
  ) => Promise<SerialDebugSessionPage>,
  historyBudget = 1200,
) {
  const reportError = vi.fn();
  const state = useSerialDebugHistory({
    readSessionPage,
    archiveLineId: (lineNo) => lineNo + 100_000,
    historyBudget: () => historyBudget,
    reportError,
  });
  return { ...state, reportError };
}

describe("useSerialDebugHistory", () => {
  it("does not apply a tail range that resolves after exit", async () => {
    const range = deferred<SerialDebugSessionPage>();
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1) return page(start, limit, 2000);
      if (start === 800) return range.promise;
      return page(start, limit, 2000);
    });
    const entering = h.enterHistoryMode(20);
    await Promise.resolve();
    h.exitHistoryMode();
    range.resolve(page(800, 400, 2000));

    expect(await entering).toBe(false);
    expect(h.historyLines.value).toEqual([]);
    expect(h.historyTotalLines.value).toBe(0);
    expect(h.historyMode.value).toBe(false);
  });

  it("does not let a stale probe repopulate session totals after exit", async () => {
    const probe = deferred<SerialDebugSessionPage>();
    const h = history(async () => probe.promise);
    const entering = h.enterHistoryMode(0);
    h.exitHistoryMode();
    probe.resolve(page(0, 1, 900));
    expect(await entering).toBe(false);
    expect(h.historyTotalLines.value).toBe(0);
    expect(h.historyLines.value).toEqual([]);
  });

  it("does not apply a tail range from the prior session after clear", async () => {
    const oldRange = deferred<SerialDebugSessionPage>();
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1) return page(start, limit, 2000);
      if (start === 800) return oldRange.promise;
      return page(start, limit, 2000);
    });
    const oldEntry = h.enterHistoryMode(5);
    await Promise.resolve();
    h.exitHistoryMode();
    oldRange.resolve(page(800, 400, 2000));

    expect(await oldEntry).toBe(false);
    expect(h.historyLines.value).toEqual([]);
    expect(h.historyTotalLines.value).toBe(0);
    expect(await h.enterHistoryMode(2)).toBe(true);
    expect(h.historyStartLineNo.value).toBe(801);
  });

  it("does not let an old request's finally clear a newer request's loading state", async () => {
    const oldRange = deferred<SerialDebugSessionPage>();
    const newRange = deferred<SerialDebugSessionPage>();
    let session: "old" | "new" = "old";
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1) {
        return page(start, limit, session === "old" ? 2000 : 3);
      }
      if (session === "old" && start === 800) return oldRange.promise;
      if (start === 0 && limit === 3) return newRange.promise;
      return page(start, limit, session === "old" ? 2000 : 3);
    });
    const oldEntry = h.enterHistoryMode(0);
    await Promise.resolve();
    h.exitHistoryMode();
    session = "new";
    const newEntry = h.enterHistoryMode(0);
    await Promise.resolve();
    expect(h.historyLoading.value).toBe(true);

    oldRange.resolve(page(800, 400, 2000));
    expect(await oldEntry).toBe(false);
    expect(h.historyLoading.value).toBe(true);

    newRange.resolve(page(0, 3, 3));
    expect(await newEntry).toBe(true);
    expect(h.historyLoading.value).toBe(false);
    expect(h.historyTotalLines.value).toBe(3);
  });

  it("ignores an old session rejection without exiting or reporting against the new session", async () => {
    const oldRange = deferred<SerialDebugSessionPage>();
    const newRange = deferred<SerialDebugSessionPage>();
    let session: "old" | "new" = "old";
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1)
        return page(start, limit, session === "old" ? 2000 : 3);
      if (session === "old" && start === 800) return oldRange.promise;
      if (session === "new" && start === 0) return newRange.promise;
      return page(start, limit, 3);
    });
    const oldEntry = h.enterHistoryMode(0);
    await Promise.resolve();
    h.exitHistoryMode();
    session = "new";
    const newEntry = h.enterHistoryMode(0);
    await Promise.resolve();
    newRange.resolve(page(0, 3, 3));
    expect(await newEntry).toBe(true);

    oldRange.reject(new Error("old archive closed"));
    expect(await oldEntry).toBe(false);
    expect(h.historyMode.value).toBe(true);
    expect(h.historyLines.value.map((line) => line.text)).toEqual([
      "line-1",
      "line-2",
      "line-3",
    ]);
    expect(h.reportError).not.toHaveBeenCalled();
  });

  it("clears loading and exits when the active session read fails", async () => {
    const range = deferred<SerialDebugSessionPage>();
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1) return page(start, limit, 2000);
      if (start === 800) return range.promise;
      return page(start, limit, 2000);
    });
    const entering = h.enterHistoryMode(0);
    await Promise.resolve();
    range.reject(new Error("active archive failed"));
    expect(await entering).toBe(false);
    expect(h.reportError).toHaveBeenCalledWith("active archive failed");
    expect(h.historyMode.value).toBe(false);
    expect(h.historyLoading.value).toBe(false);
  });

  it("reports a failed entry read and leaves history mode off", async () => {
    const h = history(async () => {
      throw new Error("archive unavailable");
    });
    expect(await h.enterHistoryMode(10)).toBe(false);
    expect(h.reportError).toHaveBeenCalledWith("archive unavailable");
    expect(h.historyMode.value).toBe(false);
    expect(h.historyLoading.value).toBe(false);
  });

  it("converts a non-Error rejection to a user-readable message", async () => {
    const h = history(async () => Promise.reject("read denied"));
    expect(await h.enterHistoryMode(0)).toBe(false);
    expect(h.reportError).toHaveBeenCalledWith("read denied");
  });

  it("does not enter an empty archive", async () => {
    const h = history(async () => page(0, 1, 0));
    expect(await h.enterHistoryMode(10)).toBe(false);
    expect(h.historyLines.value).toEqual([]);
    expect(h.historyTotalLines.value).toBe(0);
  });

  it("keeps each archive line's stable display id", async () => {
    const h = history(async (start, limit) => page(start, limit, 2));
    await h.enterHistoryMode(2);
    expect(h.historyLines.value.map((line) => line.id)).toEqual([
      100_001, 100_002,
    ]);
  });

  it("clamps the entry offset to the last available row", async () => {
    const h = history(async (start, limit) => page(start, limit, 3));
    await h.enterHistoryMode(0);
    expect(h.historyEntryOffsetLines.value).toBe(2);
  });

  it("enforces at least one archive page as the history budget", async () => {
    const h = history(
      async (start, limit) => page(start, limit, HISTORY_PAGE_SIZE + 30),
      1,
    );
    await h.enterHistoryMode(0);
    expect(h.historyLines.value).toHaveLength(HISTORY_PAGE_SIZE);
  });

  it("refuses a second entry while an entry read is pending", async () => {
    const gate = deferred<SerialDebugSessionPage>();
    const h = history(async (_start, limit) =>
      gate.promise.then(() => page(0, limit, 1)),
    );
    const first = h.enterHistoryMode(0);
    expect(await h.enterHistoryMode(0)).toBe(false);
    gate.resolve(page(0, 1, 1));
    expect(await first).toBe(true);
  });

  it("does not load older pages outside history mode", async () => {
    const read = vi.fn(async () => page(0, 1, 2));
    const h = history(read);
    expect(await h.loadOlderHistory()).toEqual({
      prepended: 0,
      reanchored: false,
    });
    expect(read).not.toHaveBeenCalled();
  });

  it("marks session start when an older page is empty", async () => {
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1) return page(start, limit, 2000);
      if (start === 400) return { ...page(start, limit, 2000), items: [] };
      return page(start, limit, 2000);
    });
    await h.enterHistoryMode(0);
    expect(await h.loadOlderHistory()).toEqual({
      prepended: 0,
      reanchored: false,
    });
    expect(h.historyAtSessionStart.value).toBe(true);
  });

  it("prepends only a continuous older page", async () => {
    const h = history(async (start, limit) => page(start, limit, 2000));
    await h.enterHistoryMode(0);
    const before = h.historyStartLineNo.value;
    const result = await h.loadOlderHistory();
    expect(result).toEqual({ prepended: HISTORY_PAGE_SIZE, reanchored: false });
    expect(h.historyStartLineNo.value).toBe(before - HISTORY_PAGE_SIZE);
  });

  it("reanchors after an older page breaks archive continuity", async () => {
    const h = history(async (start, limit) => {
      if (start === 400)
        return { ...page(start, limit, 2000), items: lines(start + 5, limit) };
      return page(start, limit, 2000);
    });
    await h.enterHistoryMode(0);
    expect(await h.loadOlderHistory()).toEqual({
      prepended: 0,
      reanchored: true,
    });
    expect(h.historyEndLineNo.value).toBe(2000);
  });

  it("marks archive end when a newer read returns no lines", async () => {
    const h = history(async (start, limit) => {
      if (start === 1200) return { ...page(start, limit, 2000), items: [] };
      return page(start, limit, 2000);
    });
    await h.enterHistoryMode(0);
    await h.jumpToSessionStart();
    expect(h.historyAtArchiveEnd.value).toBe(false);
    expect(await h.loadNewerHistory()).toBe(0);
    expect(h.historyAtArchiveEnd.value).toBe(true);
  });

  it("ignores a jump-to-start response after exit", async () => {
    const range = deferred<SerialDebugSessionPage>();
    const h = history(async (start, limit) => {
      if (start === 0 && limit === 1) return page(start, limit, 2000);
      if (start === 0) return range.promise;
      return page(start, limit, 2000);
    });
    await h.enterHistoryMode(0);
    const jumping = h.jumpToSessionStart();
    await Promise.resolve();
    h.exitHistoryMode();
    range.resolve(page(0, 400, 2000));
    expect(await jumping).toBe(false);
    expect(h.historyLines.value).toEqual([]);
    expect(h.historyLoading.value).toBe(false);
  });
});
