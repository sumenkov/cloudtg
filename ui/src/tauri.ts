import { invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export function isTauri(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof (window as any).__TAURI_INTERNALS__ === "object" &&
    typeof (window as any).__TAURI_INTERNALS__?.invoke === "function"
  );
}

export async function invokeSafe<T>(cmd: string, args?: Record<string, any>): Promise<T> {
  if (!isTauri()) {
    throw new Error("Tauri API недоступны в браузере. Запусти приложение через Tauri.");
  }
  return invoke<T>(cmd, args as any);
}

export async function listenSafe<T>(event: string, handler: EventCallback<T>): Promise<UnlistenFn> {
  if (!isTauri()) {
    return async () => {};
  }
  return listen<T>(event, handler);
}
