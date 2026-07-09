import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const updateDialogPath = fileURLToPath(
  new URL("./UpdateDialog.vue", import.meta.url),
);
const updateDialogSource = readFileSync(updateDialogPath, "utf8");

describe("UpdateDialog source actions", () => {
  it("renders a per-source update action in each source card", () => {
    expect(updateDialogSource).toContain(
      '@click="triggerSourceAction(source)"',
    );
    expect(updateDialogSource).toContain(
      't("settings.update.updateFromSource")',
    );
    expect(updateDialogSource).toContain(
      "v-if=\"sourceActionKind(source) !== 'none'\"",
    );
  });
});
