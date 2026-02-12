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

async function flushMicrotasks(times = 3) {
  for (let i = 0; i < times; i += 1) {
    await Promise.resolve();
  }
}

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

  it("drop handler uploads dropped files into selected folder", async () => {
    const prepareUploadPaths = vi.fn(async () => ["tok-1", "tok-2"]);
    const uploadFile = vi.fn(async () => {});
    const reloadOld = vi.fn(async () => {});
    const reloadNew = vi.fn(async () => {});
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };
    const reloadFilesRef = { current: reloadOld };
    const uploadInProgressRef = { current: false };
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      prepareUploadPaths,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    selectedNodeRef.current = folderNode;
    isRootSelectedRef.current = false;
    reloadFilesRef.current = reloadNew;

    handler({ payload: { type: "drop", paths: ["/tmp/one.txt", "/tmp/two.txt"] } });
    await flushMicrotasks();

    expect(prepareUploadPaths).toHaveBeenCalledWith(["/tmp/one.txt", "/tmp/two.txt"]);
    expect(uploadFile).toHaveBeenCalledTimes(2);
    expect(uploadFile).toHaveBeenNthCalledWith(1, "dir-a", "tok-1");
    expect(uploadFile).toHaveBeenNthCalledWith(2, "dir-a", "tok-2");
    expect(reloadOld).not.toHaveBeenCalled();
    expect(reloadNew).toHaveBeenCalledTimes(1);
    expect(setUploadBusy).toHaveBeenNthCalledWith(1, true);
    expect(setUploadBusy).toHaveBeenNthCalledWith(2, false);
    expect(setError).not.toHaveBeenCalled();
  });

  it("drop handler reports error when no folder is selected", async () => {
    const prepareUploadPaths = vi.fn(async () => ["tok-1"]);
    const uploadFile = vi.fn(async () => {});
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const uploadInProgressRef = { current: false };
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      prepareUploadPaths,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    handler({ payload: { type: "drop", paths: ["/tmp/file.txt"] } });
    await flushMicrotasks();

    expect(prepareUploadPaths).not.toHaveBeenCalled();
    expect(uploadFile).not.toHaveBeenCalled();
    expect(setError).toHaveBeenCalledWith("Выбери папку, чтобы загрузить файлы.");
  });

  it("drop handler handles over/leave events without starting upload", async () => {
    const prepareUploadPaths = vi.fn(async () => ["tok-1"]);
    const uploadFile = vi.fn(async () => {});
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const uploadInProgressRef = { current: false };
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      prepareUploadPaths,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    handler({ payload: { type: "over" } });
    handler({ payload: { type: "leave" } });
    handler({ payload: { type: "noop" } });
    await flushMicrotasks();

    expect(setDropActive).toHaveBeenNthCalledWith(1, true);
    expect(setDropActive).toHaveBeenNthCalledWith(2, false);
    expect(prepareUploadPaths).not.toHaveBeenCalled();
    expect(uploadFile).not.toHaveBeenCalled();
    expect(setUploadBusy).not.toHaveBeenCalled();
    expect(setError).not.toHaveBeenCalled();
  });

  it("normalizeUploadPaths trims and de-duplicates values", () => {
    const result = normalizeUploadPaths([" /a ", "/a", "", "  ", "/b"]);
    expect(result).toEqual(["/a", "/b"]);
  });
});
