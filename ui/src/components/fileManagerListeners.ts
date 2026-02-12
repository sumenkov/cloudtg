import type { DirNode } from "../store/app";

type RefValue<T> = { current: T };

type DragDropPayload = {
  type: string;
  paths?: string[];
};

type DragDropEvent = {
  payload: DragDropPayload;
};

export function normalizeUploadPaths(paths?: string[]): string[] {
  if (!paths || paths.length === 0) return [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const raw of paths) {
    const path = raw.trim();
    if (!path || seen.has(path)) continue;
    seen.add(path);
    out.push(path);
  }
  return out;
}

export function createTreeUpdatedHandler(
  selectedNodeRef: RefValue<DirNode | null>,
  isRootSelectedRef: RefValue<boolean>,
  reloadFilesRef: RefValue<() => Promise<void>>
): () => Promise<void> {
  return async () => {
    if (!selectedNodeRef.current || isRootSelectedRef.current) return;
    await reloadFilesRef.current();
  };
}

export function createDragDropHandler(
  selectedNodeRef: RefValue<DirNode | null>,
  isRootSelectedRef: RefValue<boolean>,
  setDropActive: (active: boolean) => void,
  setError: (message: string) => void
): (event: DragDropEvent) => void {
  return (event) => {
    const payload = event.payload;
    if (payload.type === "over") {
      setDropActive(true);
      return;
    }
    if (payload.type === "leave") {
      setDropActive(false);
      return;
    }
    if (payload.type !== "drop") {
      return;
    }

    setDropActive(false);
    const paths = normalizeUploadPaths(payload.paths);
    if (paths.length === 0) return;

    if (!selectedNodeRef.current || isRootSelectedRef.current) {
      setError("Выбери папку, чтобы загрузить файлы.");
      return;
    }

    setError("Перетаскивание файлов отключено из соображений безопасности. Используй кнопку «Выбрать и загрузить».");
  };
}
