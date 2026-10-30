import { MAX_FAVORITE_COMMANDS, MAX_SEND_HISTORY } from "./constants";
import type { SendHistoryEntry, SendMode } from "./types";

export function sendHistoryKey(entry: SendHistoryEntry): string {
  return JSON.stringify([entry.text, entry.mode, entry.appendCrlf]);
}

export function limitSendHistory(
  entries: SendHistoryEntry[],
): SendHistoryEntry[] {
  let recent = 0;
  let favorites = 0;
  return entries.filter((entry) =>
    entry.favorite
      ? ++favorites <= MAX_FAVORITE_COMMANDS
      : ++recent <= MAX_SEND_HISTORY,
  );
}

export function parseSendHistory(raw: unknown): SendHistoryEntry[] {
  if (!Array.isArray(raw)) return [];
  const seen = new Set<string>();
  const entries: SendHistoryEntry[] = [];
  for (const item of raw) {
    const rec = typeof item === "string" ? { text: item } : item;
    if (
      !rec ||
      typeof rec !== "object" ||
      typeof rec.text !== "string" ||
      !rec.text
    )
      continue;
    const knownMode = rec.mode === "ascii" || rec.mode === "hex";
    const knownTail = typeof rec.appendCrlf === "boolean";
    const entry: SendHistoryEntry = {
      text: rec.text,
      mode: knownMode && knownTail ? rec.mode : null,
      appendCrlf:
        knownMode && knownTail
          ? rec.mode === "hex"
            ? false
            : rec.appendCrlf
          : null,
      lastSentAt:
        Number.isFinite(rec.lastSentAt) &&
        rec.lastSentAt > 0 &&
        rec.lastSentAt <= 8.64e15
          ? rec.lastSentAt
          : null,
      sendCount:
        Number.isSafeInteger(rec.sendCount) && rec.sendCount >= 0
          ? rec.sendCount
          : 0,
      favorite: rec.favorite === true,
      name: typeof rec.name === "string" ? rec.name.trim().slice(0, 80) : "",
    };
    const key = sendHistoryKey(entry);
    if (seen.has(key)) continue;
    seen.add(key);
    entries.push(entry);
  }
  return limitSendHistory(entries);
}

export function recordSendHistory(
  entries: SendHistoryEntry[],
  text: string,
  mode: SendMode,
  appendCrlf: boolean,
  now: number,
): SendHistoryEntry[] {
  const entry: SendHistoryEntry = {
    text,
    mode,
    appendCrlf: mode === "hex" ? false : appendCrlf,
    lastSentAt: now,
    sendCount: 1,
    favorite: false,
    name: "",
  };
  const key = sendHistoryKey(entry);
  const previous = entries.find((item) => sendHistoryKey(item) === key);
  if (previous) {
    entry.sendCount = previous.sendCount + 1;
    entry.favorite = previous.favorite;
    entry.name = previous.name;
  }
  return limitSendHistory([
    entry,
    ...entries.filter((item) => sendHistoryKey(item) !== key),
  ]);
}
