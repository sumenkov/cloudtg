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

  it("drop handler uploads into latest selected folder", async () => {
    const uploadFile = vi.fn(async () => {});
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const reloadOld = vi.fn(async () => {});
    const reloadNew = vi.fn(async () => {});
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };
    const reloadFilesRef = { current: reloadOld };
    const uploadInProgressRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
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

    expect(uploadFile).toHaveBeenCalledTimes(2);
    expect(uploadFile).toHaveBeenNthCalledWith(1, "dir-a", "/tmp/one.txt");
    expect(uploadFile).toHaveBeenNthCalledWith(2, "dir-a", "/tmp/two.txt");
    expect(reloadOld).not.toHaveBeenCalled();
    expect(reloadNew).toHaveBeenCalledTimes(1);
    expect(setUploadBusy).toHaveBeenNthCalledWith(1, true);
    expect(setUploadBusy).toHaveBeenNthCalledWith(2, false);
    expect(setError).not.toHaveBeenCalled();
    expect(uploadInProgressRef.current).toBe(false);
  });

  it("drop handler reports error when no folder is selected", async () => {
    const uploadFile = vi.fn(async () => {});
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const selectedNodeRef = { current: rootNode as DirNode | null };
    const isRootSelectedRef = { current: true };
    const uploadInProgressRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    handler({ payload: { type: "drop", paths: ["/tmp/file.txt"] } });
    await flushMicrotasks();

    expect(uploadFile).not.toHaveBeenCalled();
    expect(setError).toHaveBeenCalledWith("Выбери папку, чтобы загрузить файлы.");
    expect(setUploadBusy).not.toHaveBeenCalled();
  });

  it("drop handler ignores duplicate and blank paths", async () => {
    const uploadFile = vi.fn(async () => {});
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };
    const uploadInProgressRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    handler({
      payload: { type: "drop", paths: [" /tmp/a.txt ", "/tmp/a.txt", " ", "/tmp/b.txt"] }
    });
    await flushMicrotasks();

    expect(uploadFile).toHaveBeenCalledTimes(2);
    expect(uploadFile).toHaveBeenNthCalledWith(1, "dir-a", "/tmp/a.txt");
    expect(uploadFile).toHaveBeenNthCalledWith(2, "dir-a", "/tmp/b.txt");
  });

  it("drop handler skips new drop while upload is in progress", async () => {
    let resolveFirstUpload!: () => void;
    const firstUpload = new Promise<void>((resolve) => {
      resolveFirstUpload = resolve;
    });
    let callIndex = 0;
    const uploadFile = vi.fn(async () => {
      callIndex += 1;
      if (callIndex === 1) {
        await firstUpload;
      }
    });
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };
    const uploadInProgressRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    handler({ payload: { type: "drop", paths: ["/tmp/one.txt"] } });
    handler({ payload: { type: "drop", paths: ["/tmp/two.txt"] } });

    await flushMicrotasks();
    resolveFirstUpload();
    await flushMicrotasks();

    expect(uploadFile).toHaveBeenCalledTimes(1);
    expect(uploadFile).toHaveBeenCalledWith("dir-a", "/tmp/one.txt");
    expect(setError).not.toHaveBeenCalled();
  });

  it("drop handler releases lock and busy flag after upload error", async () => {
    const uploadFile = vi.fn(async () => {
      throw new Error("upload failed");
    });
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };
    const uploadInProgressRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      uploadFile,
      setDropActive,
      setUploadBusy,
      setError
    );

    handler({ payload: { type: "drop", paths: ["/tmp/one.txt"] } });
    await flushMicrotasks();

    expect(uploadFile).toHaveBeenCalledTimes(1);
    expect(setUploadBusy).toHaveBeenNthCalledWith(1, true);
    expect(setUploadBusy).toHaveBeenNthCalledWith(2, false);
    expect(setError).toHaveBeenCalledWith("Error: upload failed");
    expect(uploadInProgressRef.current).toBe(false);
  });

  it("drop handler handles over/leave events without starting upload", async () => {
    const uploadFile = vi.fn(async () => {});
    const setDropActive = vi.fn();
    const setUploadBusy = vi.fn();
    const setError = vi.fn();
    const reloadFilesRef = { current: vi.fn(async () => {}) };
    const selectedNodeRef = { current: folderNode as DirNode | null };
    const isRootSelectedRef = { current: false };
    const uploadInProgressRef = { current: false };

    const handler = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
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
    expect(uploadFile).not.toHaveBeenCalled();
    expect(setUploadBusy).not.toHaveBeenCalled();
    expect(setError).not.toHaveBeenCalled();
  });

  it("normalizeUploadPaths trims and de-duplicates values", () => {
    const result = normalizeUploadPaths([" /a ", "/a", "", "  ", "/b"]);
    expect(result).toEqual(["/a", "/b"]);
  });
});
