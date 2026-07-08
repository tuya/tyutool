import type { LatestJson } from "./update-sources";

export type UpdateDialogSourceStatus =
  | "idle"
  | "checking"
  | "available"
  | "upToDate"
  | "failed";

export interface UpdateDialogSourceState {
  id: "github" | "gitee";
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
