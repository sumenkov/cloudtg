import { describe, expect, it, vi } from "vitest";
import {
  createAuthStateChangedHandler,
  createListenerRegistrar,
  disposeListeners,
  runSyncOnce
} from "../pages/appLifecycle";

describe("appLifecycle", () => {
  it("registrar stores unlisten callbacks while effect is active", async () => {
    const unlisten = vi.fn();
    const listen = vi.fn(async () => unlisten);
    const disposedRef = { current: false };
    const unlisteners: Array<() => void> = [];
    const addListener = createListenerRegistrar(listen, disposedRef, unlisteners);

    await addListener("tree_updated", async () => {});

    expect(listen).toHaveBeenCalledTimes(1);
    expect(unlisteners).toHaveLength(1);
    expect(unlisten).not.toHaveBeenCalled();
  });

  it("registrar cleans up listener if effect is disposed before listen resolves", async () => {
    const unlisten = vi.fn();
    let resolveListen!: (value: () => void) => void;
    const listen = vi.fn(
      () =>
        new Promise<() => void>((resolve) => {
          resolveListen = resolve;
        })
    );
    const disposedRef = { current: false };
    const unlisteners: Array<() => void> = [];
    const addListener = createListenerRegistrar(listen, disposedRef, unlisteners);

    const pending = addListener("auth_state_changed", async () => {});
    disposedRef.current = true;
    resolveListen(unlisten);
    await pending;

    expect(unlisten).toHaveBeenCalledTimes(1);
    expect(unlisteners).toHaveLength(0);
  });

  it("runSyncOnce executes sync commands only once", async () => {
    const syncStartedRef = { current: false };
    const invoke = vi.fn(async () => undefined);
    const refreshTree = vi.fn(async () => {});
    const setError = vi.fn();

    await runSyncOnce({ syncStartedRef, invoke, refreshTree, setError });
    await runSyncOnce({ syncStartedRef, invoke, refreshTree, setError });

    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke).toHaveBeenNthCalledWith(1, "tg_sync_storage");
    expect(invoke).toHaveBeenNthCalledWith(2, "tg_reconcile_recent", { limit: 100 });
    expect(refreshTree).toHaveBeenCalledTimes(1);
    expect(setError).not.toHaveBeenCalled();
  });

  it("runSyncOnce reports sync error when not disposed", async () => {
    const syncStartedRef = { current: false };
    const invoke = vi.fn(async () => {
      throw new Error("sync failed");
    });
    const refreshTree = vi.fn(async () => {});
    const setError = vi.fn();

    await runSyncOnce({ syncStartedRef, invoke, refreshTree, setError, disposedRef: { current: false } });

    expect(setError).toHaveBeenCalledWith("Error: sync failed");
    expect(refreshTree).not.toHaveBeenCalled();
  });

  it("auth_state_changed handler refreshes tree and starts sync once", async () => {
    const disposedRef = { current: false };
    const syncStartedRef = { current: false };
    const setAuth = vi.fn();
    const refreshTree = vi.fn(async () => {});
    const invoke = vi.fn(async () => undefined);
    const setError = vi.fn();
    const handler = createAuthStateChangedHandler({
      disposedRef,
      syncStartedRef,
      setAuth,
      refreshTree,
      invoke,
      setError
    });

    await handler({ payload: { state: "ready" } });
    await handler({ payload: { state: "ready" } });
    await handler({ payload: { state: "wait_code" } });

    expect(setAuth).toHaveBeenCalledTimes(3);
    expect(setAuth).toHaveBeenNthCalledWith(1, "ready");
    expect(setAuth).toHaveBeenNthCalledWith(2, "ready");
    expect(setAuth).toHaveBeenNthCalledWith(3, "wait_code");
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(refreshTree).toHaveBeenCalledTimes(3);
    expect(setError).not.toHaveBeenCalled();
  });

  it("auth_state_changed handler ignores events when disposed", async () => {
    const setAuth = vi.fn();
    const refreshTree = vi.fn(async () => {});
    const invoke = vi.fn(async () => undefined);
    const setError = vi.fn();
    const handler = createAuthStateChangedHandler({
      disposedRef: { current: true },
      syncStartedRef: { current: false },
      setAuth,
      refreshTree,
      invoke,
      setError
    });

    await handler({ payload: { state: "ready" } });

    expect(setAuth).not.toHaveBeenCalled();
    expect(refreshTree).not.toHaveBeenCalled();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("disposeListeners flips disposed flag and calls all cleanups", () => {
    const disposedRef = { current: false };
    const unlistenA = vi.fn();
    const unlistenB = vi.fn();
    const unlisteners = [unlistenA, unlistenB];

    disposeListeners(disposedRef, unlisteners);

    expect(disposedRef.current).toBe(true);
    expect(unlistenA).toHaveBeenCalledTimes(1);
    expect(unlistenB).toHaveBeenCalledTimes(1);
  });
});
