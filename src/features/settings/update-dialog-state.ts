import type { LatestJson } from "./update-sources";

export type UpdateDialogSourceStatus =
  | "idle"
  | "checking"
  | "available"
  | "upToDate"
  | "failed";

export interface UpdateDialogSourceState {
  id: "github" | "tuya";
  labelKey: string;
  status: UpdateDialogSourceStatus;
  version: string;
  elapsed: number;
  manifest: LatestJson | null;
  error: string;
}

export type UpdateSummaryKind =
  | "checking"
  | "available"
  | "upToDate"
  | "failed"
  | "downloading"
  | "ready"
  | "installing";

export interface UpdateSummaryState {
  kind: UpdateSummaryKind;
  availableSource: UpdateDialogSourceState | null;
  failedCount: number;
  completedCount: number;
}

export type UpdateSourceActionKind = "none" | "download" | "manual";

export function deriveUpdateSummaryState(input: {
  sourceStates: UpdateDialogSourceState[];
  downloading: boolean;
  downloadReady: boolean;
  installing: boolean;
}): UpdateSummaryState {
  const { sourceStates, downloading, downloadReady, installing } = input;
  const availableSource =
    sourceStates.find((source) => source.status === "available") ?? null;
  const failedCount = sourceStates.filter(
    (source) => source.status === "failed",
  ).length;
  const completedCount = sourceStates.filter(
    (source) => source.status !== "checking" && source.status !== "idle",
  ).length;
  const hasCheckingSource = sourceStates.some(
    (source) => source.status === "checking" || source.status === "idle",
  );
  const hasUpToDateSource = sourceStates.some(
    (source) => source.status === "upToDate",
  );
  const allFailed =
    sourceStates.length > 0 && failedCount === sourceStates.length;

  if (installing) {
    return {
      kind: "installing",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  if (downloadReady) {
    return {
      kind: "ready",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  if (downloading) {
    return {
      kind: "downloading",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  if (availableSource) {
    return {
      kind: "available",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  if (hasCheckingSource) {
    return {
      kind: "checking",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  if (hasUpToDateSource) {
    return {
      kind: "upToDate",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  if (allFailed) {
    return {
      kind: "failed",
      availableSource,
      failedCount,
      completedCount,
    };
  }

  return {
    kind: "checking",
    availableSource,
    failedCount,
    completedCount,
  };
}

export function deriveUpdateSourceAction(input: {
  source: UpdateDialogSourceState;
  summaryKind: UpdateSummaryKind;
  isTauri: boolean;
  installTypeReady: boolean;
  manualUpdateOnly: boolean;
  inAppUpdateSupported: boolean;
  primaryAvailableSourceId: UpdateDialogSourceState["id"] | null;
}): UpdateSourceActionKind {
  const {
    source,
    summaryKind,
    isTauri,
    installTypeReady,
    manualUpdateOnly,
    inAppUpdateSupported,
    primaryAvailableSourceId,
  } = input;

  if (
    summaryKind !== "available" ||
    source.status !== "available" ||
    !isTauri ||
    !installTypeReady
  ) {
    return "none";
  }

  if (manualUpdateOnly) {
    return "manual";
  }

  if (inAppUpdateSupported && primaryAvailableSourceId === source.id) {
    return "download";
  }

  return "manual";
}
