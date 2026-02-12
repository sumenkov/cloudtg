import { describe, expect, it, vi } from "vitest";
import type { DirNode } from "../store/app";
import {
  createDragDropHandler,
  createTreeUpdatedHandler,
  normalizeUploadPaths
} from "../components/fileManagerListeners";

const rootNode: DirNode = {
  id: "ROOT",
  name: "ROOT",
  parent_id: null,
  is_broken: false,
  children: []
};

const folderNode: DirNode = {
  id: "dir-a",
  name: "Folder A",
  parent_id: "ROOT",
  is_broken: false,
  children: []
};

describe("fileManagerListeners", () => {
  it("tree_updated handler reads latest refs at call time", async () => {
    const reloadOld = vi.fn(async () => {});
    const reloadNew = vi.fn(async () => {});
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };
    const reloadFilesRef = { current: reloadOld };

    const handler = createTreeUpdatedHandler(selectedNodeRef, isRootSelectedRef, reloadFilesRef);

    selectedNodeRef.current = folderNode;
    isRootSelectedRef.current = false;
    reloadFilesRef.current = reloadNew;

    await handler();

    expect(reloadOld).not.toHaveBeenCalled();
    expect(reloadNew).toHaveBeenCalledTimes(1);
  });

  it("tree_updated handler skips reload for root or empty selection", async () => {
    const reloadFiles = vi.fn(async () => {});
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };
    const reloadFilesRef = { current: reloadFiles };

    const handler = createTreeUpdatedHandler(selectedNodeRef, isRootSelectedRef, reloadFilesRef);
    await handler();

    selectedNodeRef.current = null;
    isRootSelectedRef.current = false;
    await handler();

    expect(reloadFiles).not.toHaveBeenCalled();
  });

  it("drop handler blocks drag and drop uploads for security", () => {
    const setDropActive = vi.fn();
    const setError = vi.fn();
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      setDropActive,
      setError
    );

    handler({ payload: { type: "drop", paths: ["/tmp/one.txt", "/tmp/two.txt"] } });
    expect(setError).toHaveBeenCalledWith(
      "Перетаскивание файлов отключено из соображений безопасности. Используй кнопку «Выбрать и загрузить»."
    );
  });

  it("drop handler reports error when no folder is selected", () => {
    const setDropActive = vi.fn();
    const setError = vi.fn();
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      setDropActive,
      setError
    );

    handler({ payload: { type: "drop", paths: ["/tmp/file.txt"] } });
    expect(setError).toHaveBeenCalledWith("Выбери папку, чтобы загрузить файлы.");
  });

  it("drop handler handles over/leave events without reporting errors", () => {
    const setDropActive = vi.fn();
    const setError = vi.fn();
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      setDropActive,
      setError
    );

    handler({ payload: { type: "over" } });
    handler({ payload: { type: "leave" } });
    handler({ payload: { type: "noop" } });

    expect(setDropActive).toHaveBeenNthCalledWith(1, true);
    expect(setDropActive).toHaveBeenNthCalledWith(2, false);
    expect(setError).not.toHaveBeenCalled();
  });

  it("normalizeUploadPaths trims and de-duplicates values", () => {
    const result = normalizeUploadPaths([" /a ", "/a", "", "  ", "/b"]);
    expect(result).toEqual(["/a", "/b"]);
  });
});
