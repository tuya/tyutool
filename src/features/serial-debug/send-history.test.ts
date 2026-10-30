import { describe, expect, it } from "vitest";
import {
  parseSendHistory,
  recordSendHistory,
  sendHistoryKey,
} from "./send-history";
import { MAX_SEND_HISTORY } from "./constants";

describe("serial command history", () => {
  it("migrates text-only history without guessing mode, line endings or time", () => {
    const entries = parseSendHistory(["AT", 12, null, "AT", "AA"]);
    expect(entries).toHaveLength(2);
    expect(entries[0]).toMatchObject({
      text: "AT",
      mode: null,
      appendCrlf: null,
      lastSentAt: null,
      sendCount: 0,
    });
  });

  it("keeps different send settings distinct and preserves named favorites on resend", () => {
    let entries = recordSendHistory([], "AT", "ascii", true, 100);
    entries[0] = { ...entries[0], favorite: true, name: "Version" };
    entries = recordSendHistory(entries, "AT", "ascii", false, 200);
    entries = recordSendHistory(entries, "AT", "hex", true, 300);
    entries = recordSendHistory(entries, "AT", "ascii", true, 400);
    expect(entries).toHaveLength(3);
    expect(entries[0]).toMatchObject({
      mode: "ascii",
      appendCrlf: true,
      sendCount: 2,
      lastSentAt: 400,
      favorite: true,
      name: "Version",
    });
    expect(entries[1]).toMatchObject({ mode: "hex", appendCrlf: false });
    expect(new Set(entries.map(sendHistoryKey)).size).toBe(3);
  });

  it("retains favorites when old recent commands are evicted", () => {
    let entries = recordSendHistory([], "PINNED", "ascii", false, 1);
    entries[0].favorite = true;
    for (let i = 0; i < MAX_SEND_HISTORY + 10; i++)
      entries = recordSendHistory(entries, String(i), "ascii", false, i + 2);
    expect(entries.filter((entry) => !entry.favorite)).toHaveLength(
      MAX_SEND_HISTORY,
    );
    expect(entries[entries.length - 1]?.text).toBe("PINNED");
  });

  it("repairs malformed records and preserves valid metadata across reload", () => {
    const valid = recordSendHistory([], "55 AA", "hex", false, 1000)[0];
    const parsed = parseSendHistory([
      { ...valid, favorite: true, name: "Query" },
      {
        text: "broken",
        mode: "binary",
        appendCrlf: "yes",
        lastSentAt: Infinity,
        sendCount: -2,
      },
      { text: 5 },
    ]);
    expect(parsed[0]).toEqual({ ...valid, favorite: true, name: "Query" });
    expect(parsed[1]).toMatchObject({
      mode: null,
      appendCrlf: null,
      lastSentAt: null,
      sendCount: 0,
    });
    expect(parsed).toHaveLength(2);
  });
});
