import { describe, expect, it } from "vitest";
import {
  deriveUpdateSourceAction,
  deriveUpdateSummaryState,
  type UpdateDialogSourceState,
} from "./update-dialog-state";

function makeSourceState(
  status: UpdateDialogSourceState["status"],
  overrides: Partial<UpdateDialogSourceState> = {},
): UpdateDialogSourceState {
  return {
    id: "github",
    labelKey: "settings.update.sourceGithub",
    status,
    version: "",
    elapsed: 0,
    manifest: null,
    error: "",
    ...overrides,
  };
}

describe("deriveUpdateSummaryState", () => {
  it("prefers an available update over up-to-date and failed sources", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [
        makeSourceState("failed", { error: "timeout" }),
        makeSourceState("available", { version: "3.3.0" }),
      ],
      downloading: false,
      downloadReady: false,
      installing: false,
    });

    expect(result.kind).toBe("available");
    expect(result.availableSource?.version).toBe("3.3.0");
    expect(result.failedCount).toBe(1);
  });

  it("reports up-to-date when at least one source succeeds and none have updates", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [
        makeSourceState("upToDate", { elapsed: 0.9 }),
        makeSourceState("failed", { error: "network" }),
      ],
      downloading: false,
      downloadReady: false,
      installing: false,
    });

    expect(result.kind).toBe("upToDate");
    expect(result.failedCount).toBe(1);
    expect(result.completedCount).toBe(2);
  });

  it("reports failed when all sources fail", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [
        makeSourceState("failed", { error: "timeout" }),
        makeSourceState("failed", { error: "blocked" }),
      ],
      downloading: false,
      downloadReady: false,
      installing: false,
    });

    expect(result.kind).toBe("failed");
    expect(result.failedCount).toBe(2);
  });

  it("keeps checking while a source is still in flight and no higher-priority state exists", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [
        makeSourceState("checking"),
        makeSourceState("failed", { error: "timeout" }),
      ],
      downloading: false,
      downloadReady: false,
      installing: false,
    });

    expect(result.kind).toBe("checking");
    expect(result.failedCount).toBe(1);
  });

  it("lets downloading override source availability", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [makeSourceState("available", { version: "3.3.0" })],
      downloading: true,
      downloadReady: false,
      installing: false,
    });

    expect(result.kind).toBe("downloading");
  });

  it("lets ready override downloading", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [makeSourceState("available", { version: "3.3.0" })],
      downloading: true,
      downloadReady: true,
      installing: false,
    });

    expect(result.kind).toBe("ready");
  });

  it("lets installing override every other state", () => {
    const result = deriveUpdateSummaryState({
      sourceStates: [makeSourceState("available", { version: "3.3.0" })],
      downloading: true,
      downloadReady: true,
      installing: true,
    });

    expect(result.kind).toBe("installing");
  });
});

describe("deriveUpdateSourceAction", () => {
  it("uses in-app download for the primary available source when supported", () => {
    const result = deriveUpdateSourceAction({
      source: makeSourceState("available", { id: "github" }),
      summaryKind: "available",
      isTauri: true,
      installTypeReady: true,
      manualUpdateOnly: false,
      inAppUpdateSupported: true,
      primaryAvailableSourceId: "github",
    });

    expect(result).toBe("download");
  });

  it("falls back to the selected source release page for non-primary sources", () => {
    const result = deriveUpdateSourceAction({
      source: makeSourceState("available", { id: "tuya" }),
      summaryKind: "available",
      isTauri: true,
      installTypeReady: true,
      manualUpdateOnly: false,
      inAppUpdateSupported: true,
      primaryAvailableSourceId: "github",
    });

    expect(result).toBe("manual");
  });

  it("uses the source release page when the install type only supports manual updates", () => {
    const result = deriveUpdateSourceAction({
      source: makeSourceState("available", { id: "github" }),
      summaryKind: "available",
      isTauri: true,
      installTypeReady: true,
      manualUpdateOnly: true,
      inAppUpdateSupported: false,
      primaryAvailableSourceId: "github",
    });

    expect(result).toBe("manual");
  });

  it("hides source actions outside the available state", () => {
    const result = deriveUpdateSourceAction({
      source: makeSourceState("available", { id: "github" }),
      summaryKind: "downloading",
      isTauri: true,
      installTypeReady: true,
      manualUpdateOnly: false,
      inAppUpdateSupported: true,
      primaryAvailableSourceId: "github",
    });

    expect(result).toBe("none");
  });
});
