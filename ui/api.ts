import { invoke, isTauri } from "@tauri-apps/api/core";
export const native = isTauri();
export async function api<T>(op: string, args: unknown = {}): Promise<T> {
  if (!native)
    throw new Error(
      "Откройте настольное приложение. Браузер не имеет доступа к локальному индексу.",
    );
  return invoke<T>("api", { op, args });
}
export interface Root {
  id: number;
  path: string;
  online: boolean;
  error: string | null;
}
export interface Hit {
  id: number;
  name: string;
  path: string;
  size: number;
  modified: number;
  source: string;
  location: string;
  snippet: string;
  online: boolean;
}
export interface Block {
  location: string;
  source: string;
  text: string;
}
export interface Status {
  total: number;
  pending: number;
  processing: number;
  ready: number;
  errors: number;
  unsupported: number;
  offline: number;
  bytes: number;
  paused: boolean;
  scanning: boolean;
  cancelled: boolean;
}
export const sourceLabel: Record<string, string> = {
  name: "Имя файла",
  path: "Путь",
  text: "Текст документа",
  ocr: "Распознанный текст",
};
