import { describe, expect, it } from "vitest";
import { buildUpdateEntryModel } from "./update-entry-model";

describe("buildUpdateEntryModel", () => {
  it("uses the current app version as the badge", () => {
    expect(buildUpdateEntryModel("3.2.1").badge).toBe("v3.2.1");
  });

  it("returns the expected translation keys for the card copy", () => {
    expect(buildUpdateEntryModel("3.2.1")).toMatchObject({
      panelTitleKey: "settings.updateCenterTitle",
      panelBodyKey: "settings.updateCenterBody",
      versionLabelKey: "settings.updateCenterVersionLabel",
      titleKey: "settings.checkUpdate",
      subtitleKey: "settings.updateEntryHint",
      metaLabelKey: "settings.updateEntryMetaLabel",
    });
  });
});
