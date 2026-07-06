import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const sourcePath = fileURLToPath(
  new URL("./LogViewerDialog.vue", import.meta.url),
);
const source = readFileSync(sourcePath, "utf8");

describe("LogViewerDialog", () => {
  it("uses light-mode readable styling for the current-session badge", () => {
    expect(source).not.toContain("bg-blue-600/40");
    expect(source).not.toContain("text-blue-300");
  });
});
