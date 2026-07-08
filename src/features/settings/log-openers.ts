import { isTauriRuntime } from "@/runtime";

export interface LogFileOpener {
  id: string;
  label: string;
}

export async function listLogFileOpeners(): Promise<LogFileOpener[]> {
  if (!isTauriRuntime()) return [];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<LogFileOpener[]>("list_log_file_openers");
}

export async function openLogFileInEditor(
  filename: string,
  editorId: string,
): Promise<void> {
  if (!isTauriRuntime()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_log_file_in_editor", { filename, editorId });
}
