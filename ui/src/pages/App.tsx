import React, { useEffect, useRef, useState } from "react";
import { invokeSafe, listenSafe } from "../tauri";
import { useAppStore } from "../store/app";
import { FileManager } from "../components/FileManager";
import { Login } from "../components/Login";
import { Settings } from "../components/Settings";
import { Hint } from "../components/common/Hint";
import {
  createAuthStateChangedHandler,
  createListenerRegistrar,
  disposeListeners,
  runSyncOnce
} from "./appLifecycle";

export default function App() {
  const {
    auth,
    setAuth,
    tree,
    refreshTree,
    error,
    setError,
    refreshAuth,
    refreshSettings,
    setTdlibBuild,
    clearTdlibLogs,
    pushTdlibLog,
    touchTdlibBuildOnLog,
    tdlibBuild,
    tgSync,
    setTgSync
  } = useAppStore();
  const [showSettings, setShowSettings] = useState(false);
  const [autoShowSettings, setAutoShowSettings] = useState(true);
  const syncStartedRef = useRef(false);
  const progressValue =
    tdlibBuild.progress === null ? null : Math.max(0, Math.min(100, tdlibBuild.progress));
  const syncProgressValue =
    tgSync.total && tgSync.total > 0 ? Math.max(0, Math.min(100, Math.floor((tgSync.processed / tgSync.total) * 100))) : null;

  useEffect(() => {
    const disposedRef = { current: false };
    const unlisteners: Array<() => void> = [];
    const addListener = createListenerRegistrar(listenSafe, disposedRef, unlisteners);

    (async () => {
      try {
        const state = await refreshAuth();
        if (disposedRef.current) return;
        await refreshSettings();
        if (disposedRef.current) return;
        await refreshTree();
        if (disposedRef.current) return;

        if (state === "ready") {
          await runSyncOnce({
            syncStartedRef,
            invoke: invokeSafe,
            refreshTree,
            setError,
            disposedRef
          });
        }

        await addListener(
          "auth_state_changed",
          createAuthStateChangedHandler({
            disposedRef,
            syncStartedRef,
            setAuth,
            refreshTree,
            invoke: invokeSafe,
            setError
          })
        );

        await addListener<{ state: string; message: string; detail?: string | null }>("tdlib_build_status", async (event) => {
          if (disposedRef.current) return;
          if (event.payload.state === "start") {
            clearTdlibLogs();
          }
          setTdlibBuild({
            state: event.payload.state ?? null,
            message: event.payload.message ?? null,
            detail: event.payload.detail ?? null,
            progress: null
          });
        });

        await addListener<{ stream: "stdout" | "stderr"; line: string }>("tdlib_build_log", async (event) => {
          if (disposedRef.current) return;
          touchTdlibBuildOnLog();
          pushTdlibLog(event.payload.stream, event.payload.line);
        });

        await addListener<{ state: string; message: string; processed: number; total: number | null }>("tg_sync_status", async (event) => {
          if (disposedRef.current) return;
          setTgSync({
            state: event.payload.state ?? null,
            message: event.payload.message ?? null,
            processed: event.payload.processed ?? 0,
            total: event.payload.total ?? null
          });
        });

        await addListener<unknown>("tree_updated", async () => {
          if (disposedRef.current) return;
          await refreshTree();
        });
      } catch (e: any) {
        if (!disposedRef.current) {
          setError(String(e));
        }
      }
    })();

    return () => {
      disposeListeners(disposedRef, unlisteners);
    };
  }, [
    refreshAuth,
    refreshSettings,
    refreshTree,
    setAuth,
    setError,
    setTdlibBuild,
    clearTdlibLogs,
    pushTdlibLog,
    touchTdlibBuildOnLog,
    setTgSync
  ]);

  useEffect(() => {
    if (auth === "wait_config" && autoShowSettings) {
      setShowSettings(true);
    }
    if (auth === "ready") {
      setAutoShowSettings(true);
    }
  }, [auth, autoShowSettings]);

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 16, maxWidth: 1100, margin: "0 auto" }}>
      <style>
        {`@keyframes tgSyncMove { 0% { transform: translateX(-60%); } 50% { transform: translateX(60%); } 100% { transform: translateX(120%); } }`}
      </style>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h1 style={{ marginBottom: 6 }}>CloudTG</h1>
        <button
          onClick={() => {
            setShowSettings((v) => !v);
            setAutoShowSettings(false);
          }}
          style={{ padding: "8px 12px", borderRadius: 10 }}
        >
          Настройки
        </button>
      </div>
      <p style={{ marginTop: 0, opacity: 0.8 }}>
        Добро пожаловать в CloudTG. Здесь можно хранить, искать и открывать файлы из Telegram как в обычном файловом менеджере.
      </p>

      {tdlibBuild.state && tdlibBuild.state !== "success" ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: tdlibBuild.state === "error" ? "1px solid #f99" : "1px solid #ddd",
            background: tdlibBuild.state === "error" ? "#fee" : "#fafafa"
          }}
        >
          <b>Подготовка TDLib</b>
          <div style={{ marginTop: 6 }}>{tdlibBuild.message ?? "Статус сборки"}</div>
          {["start", "clone", "configure", "build", "download"].includes(tdlibBuild.state ?? "") ? (
            <div style={{ marginTop: 8, height: 8, background: "#e5e5e5", borderRadius: 999 }}>
              <div
                style={{
                  width:
                    progressValue !== null
                      ? `${progressValue}%`
                      : tdlibBuild.state === "download"
                      ? "25%"
                      : tdlibBuild.state === "start"
                      ? "15%"
                      : tdlibBuild.state === "clone"
                      ? "35%"
                      : tdlibBuild.state === "configure"
                      ? "60%"
                      : "85%",
                  height: "100%",
                  background: "#4a90e2",
                  borderRadius: 999
                }}
              />
            </div>
          ) : null}
          {progressValue !== null ? (
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>Прогресс: {progressValue}%</div>
          ) : null}
          {["start", "clone", "configure", "build", "download"].includes(tdlibBuild.state ?? "") ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Пока подготовка не закончится, программа работать не будет.
            </div>
          ) : null}
          {tdlibBuild.state === "success" ? (
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>Сборка завершена успешно.</div>
          ) : null}
          {tdlibBuild.state === "error" && tdlibBuild.detail ? (
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>{tdlibBuild.detail}</div>
          ) : null}
        </div>
      ) : null}

      {tgSync.state && ["start", "progress"].includes(tgSync.state) ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: "1px solid #ddd",
            background: "#fafafa"
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <b>Синхронизация из Telegram</b>
            <Hint text="Синхронизация может идти в фоне. Можно продолжать работу с интерфейсом." />
          </div>
          <div style={{ marginTop: 6 }}>{tgSync.message ?? "Читаю сообщения канала"}</div>
          <div style={{ marginTop: 8, height: 8, background: "#e5e5e5", borderRadius: 999, overflow: "hidden" }}>
            <div
              style={{
                width: syncProgressValue !== null ? `${syncProgressValue}%` : "40%",
                height: "100%",
                background: "#4a90e2",
                borderRadius: 999,
                animation: syncProgressValue === null ? "tgSyncMove 1.4s ease-in-out infinite" : "none"
              }}
            />
          </div>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>
            {syncProgressValue !== null ? `Прогресс: ${syncProgressValue}%` : `Обработано сообщений: ${tgSync.processed}`}
          </div>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
            Если сообщений много, синхронизация может занять время.
          </div>
        </div>
      ) : null}

      {tgSync.state === "error" ? (
        <div style={{ background: "#fee", border: "1px solid #f99", padding: 12, borderRadius: 8, marginBottom: 12 }}>
          <b>Синхронизация не удалась.</b> Проверь логи и подключение к Telegram.
        </div>
      ) : null}

      {auth === "ready" && !showSettings ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: "1px solid #ddd",
            background: "#f7f7f7",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            flexWrap: "wrap"
          }}
        >
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <b>Обновить из Telegram</b>
              <Hint text="Проверяем новые файлы и изменения в канале хранения." />
            </div>
            <div style={{ marginTop: 4, fontSize: 12, opacity: 0.7 }}>
              Считываем новые сообщения из канала и обновляем структуру файлов.
            </div>
          </div>
          <button
            onClick={async () => {
              try {
                await invokeSafe("tg_sync_storage");
                await refreshTree();
              } catch (e: any) {
                setError(String(e));
              }
            }}
            disabled={tgSync.state === "start" || tgSync.state === "progress"}
            style={{ padding: "10px 14px", borderRadius: 10 }}
          >
            Обновить сейчас
          </button>
        </div>
      ) : null}

      {auth === "ready" && !showSettings ? (
        <div style={{ marginBottom: 12, padding: 12, borderRadius: 10, border: "1px solid #eee", background: "#fcfcfc" }}>
          <b>Как начать работу</b>
          <ol style={{ marginTop: 8, marginBottom: 0, paddingLeft: 20, fontSize: 13, opacity: 0.8 }}>
            <li>Выбери папку в дереве слева.</li>
            <li>Во вкладке «Файлы» загрузи или открой нужный файл.</li>
            <li>Для поиска используй вкладку «Поиск».</li>
            <li>Чтобы отправить файл в чат, во вкладке «Файлы» открой меню «⋯ Действия» и нажми «Поделиться».</li>
          </ol>
        </div>
      ) : null}

      {error ? (
        <div style={{ background: "#fee", border: "1px solid #f99", padding: 12, borderRadius: 8 }}>
          <b>Ошибка:</b> {error}
        </div>
      ) : null}

      {showSettings ? (
        <Settings
          onClose={() => {
            setShowSettings(false);
            setAutoShowSettings(false);
          }}
        />
      ) : auth !== "ready" ? (
        <Login />
      ) : (
        <FileManager tree={tree} />
      )}
    </div>
  );
}
