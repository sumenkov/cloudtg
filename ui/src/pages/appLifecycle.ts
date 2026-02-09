type DisposedRef = { current: boolean };
type SyncStartedRef = { current: boolean };

export type AppEvent<T> = { payload: T };
export type EventHandler<T> = (event: AppEvent<T>) => Promise<void>;
export type ListenFn = <T>(event: string, handler: EventHandler<T>) => Promise<() => void>;
export type InvokeFn = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

type RunSyncOnceArgs = {
  syncStartedRef: SyncStartedRef;
  invoke: InvokeFn;
  refreshTree: () => Promise<void>;
  setError: (message: string) => void;
  disposedRef?: DisposedRef;
};

type AuthStateHandlerArgs = {
  disposedRef: DisposedRef;
  syncStartedRef: SyncStartedRef;
  setAuth: (state: string) => void;
  refreshTree: () => Promise<void>;
  invoke: InvokeFn;
  setError: (message: string) => void;
};

export function createListenerRegistrar(
  listen: ListenFn,
  disposedRef: DisposedRef,
  unlisteners: Array<() => void>
) {
  return async function addListener<T>(event: string, handler: EventHandler<T>): Promise<void> {
    const unlisten = await listen<T>(event, handler);
    // Эффект может размонтироваться до завершения async-подписки (например, в React.StrictMode).
    if (disposedRef.current) {
      unlisten();
      return;
    }
    unlisteners.push(unlisten);
  };
}

export async function runSyncOnce({
  syncStartedRef,
  invoke,
  refreshTree,
  setError,
  disposedRef
}: RunSyncOnceArgs): Promise<void> {
  if (syncStartedRef.current) {
    return;
  }
  syncStartedRef.current = true;
  try {
    await invoke("tg_sync_storage");
    await invoke("tg_reconcile_recent", { limit: 100 });
    await refreshTree();
  } catch (e: unknown) {
    if (!disposedRef || !disposedRef.current) {
      setError(String(e));
    }
  }
}

export function createAuthStateChangedHandler({
  disposedRef,
  syncStartedRef,
  setAuth,
  refreshTree,
  invoke,
  setError
}: AuthStateHandlerArgs): EventHandler<{ state: string }> {
  return async (event) => {
    if (disposedRef.current) return;
    setAuth(event.payload.state);
    if (event.payload.state === "ready") {
      await refreshTree();
      await runSyncOnce({
        syncStartedRef,
        invoke,
        refreshTree,
        setError,
        disposedRef
      });
    }
  };
}

export function disposeListeners(disposedRef: DisposedRef, unlisteners: Array<() => void>): void {
  disposedRef.current = true;
  unlisteners.forEach((fn) => fn());
}
