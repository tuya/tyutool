import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const settingsPagePath = fileURLToPath(
  new URL("./SettingsPage.vue", import.meta.url),
);
const settingsPageSource = readFileSync(settingsPagePath, "utf8");

const serialDebugSettingsModalPath = fileURLToPath(
  new URL(
    "../serial-debug/components/SerialDebugSettingsModal.vue",
    import.meta.url,
  ),
);
const serialDebugSettingsModalSource = readFileSync(
  serialDebugSettingsModalPath,
  "utf8",
);

const batchFlashAuthConfigPath = fileURLToPath(
  new URL(
    "../batch-flash-auth/components/BatchFlashAuthConfig.vue",
    import.meta.url,
  ),
);
const batchFlashAuthConfigSource = readFileSync(
  batchFlashAuthConfigPath,
  "utf8",
);

describe("settings UI structure", () => {
  it("keeps the about section free of a duplicated version block", () => {
    expect(settingsPageSource).not.toContain('t("settings.version"');
    expect(settingsPageSource).not.toContain("about-footer__version-value");
  });

  it("uses the shared TySwitch on settings-style surfaces", () => {
    expect(settingsPageSource).toContain('from "@/components/TySwitch.vue"');
    expect(settingsPageSource).not.toContain("logToggleOptions");
    expect(serialDebugSettingsModalSource).toContain(
      'from "@/components/TySwitch.vue"',
    );
    expect(batchFlashAuthConfigSource).toContain(
      'from "@/components/TySwitch.vue"',
    );
    expect(batchFlashAuthConfigSource).not.toContain('role="switch"');
  });

  it("wires the batch-auth disclaimer reset button in the About section", () => {
    expect(settingsPageSource).toContain(
      'from "@/features/batch-flash-auth/disclaimer"',
    );
    expect(settingsPageSource).toContain("DISCLAIMER_KEY");
    expect(settingsPageSource).toContain('t("settings.resetDisclaimer")');
    expect(settingsPageSource).toContain('t("settings.resetDisclaimerDone")');
  });
});
