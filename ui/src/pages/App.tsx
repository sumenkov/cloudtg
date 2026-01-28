import React, { useEffect, useRef, useState } from "react";
import { invokeSafe, listenSafe } from "../tauri";
import { useAppStore } from "../store/app";
import { FileManager } from "../components/FileManager";
import { Login } from "../components/Login";
import { Settings } from "../components/Settings";

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
    const unlisteners: Array<() => void> = [];

    (async () => {
      try {
        const state = await refreshAuth();
        await refreshSettings();
        await refreshTree();
        if (state === "ready" && !syncStartedRef.current) {
          syncStartedRef.current = true;
          try {
            await invokeSafe("tg_sync_storage");
            await refreshTree();
          } catch (e: any) {
            setError(String(e));
          }
        }

        unlisteners.push(
          await listenSafe<{ state: string }>("auth_state_changed", async (event) => {
            setAuth(event.payload.state);
            if (event.payload.state === "ready") {
              await refreshTree();
              if (!syncStartedRef.current) {
                syncStartedRef.current = true;
                try {
                  await invokeSafe("tg_sync_storage");
                  await refreshTree();
                } catch (e: any) {
                  setError(String(e));
                }
              }
            }
          })
        );

        unlisteners.push(
          await listenSafe<{ state: string; message: string; detail?: string | null }>("tdlib_build_status", async (event) => {
            if (event.payload.state === "start") {
              clearTdlibLogs();
            }
            setTdlibBuild({
              state: event.payload.state ?? null,
              message: event.payload.message ?? null,
              detail: event.payload.detail ?? null,
              progress: null
            });
          })
        );

        unlisteners.push(
          await listenSafe<{ stream: "stdout" | "stderr"; line: string }>("tdlib_build_log", async (event) => {
            touchTdlibBuildOnLog();
            pushTdlibLog(event.payload.stream, event.payload.line);
          })
        );

        unlisteners.push(
          await listenSafe<{ state: string; message: string; processed: number; total: number | null }>(
            "tg_sync_status",
            async (event) => {
              setTgSync({
                state: event.payload.state ?? null,
                message: event.payload.message ?? null,
                processed: event.payload.processed ?? 0,
                total: event.payload.total ?? null
              });
            }
          )
        );

        unlisteners.push(
          await listenSafe("tree_updated", async () => {
            await refreshTree();
          })
        );
      } catch (e: any) {
        setError(String(e));
      }
    })();

    return () => {
      unlisteners.forEach((fn) => fn());
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
        Файлы в Telegram, структура в SQLite. Авторизация через TDLib.
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
          <b>Сборка TDLib</b>
          <div style={{ marginTop: 6 }}>{tdlibBuild.message ?? "Статус сборки"}</div>
          {["start", "clone", "configure", "build"].includes(tdlibBuild.state ?? "") ? (
            <div style={{ marginTop: 8, height: 8, background: "#e5e5e5", borderRadius: 999 }}>
              <div
                style={{
                  width:
                    progressValue !== null
                      ? `${progressValue}%`
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
          {["start", "clone", "configure", "build"].includes(tdlibBuild.state ?? "") ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Пока сборка не закончится, программа работать не будет.
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
          <b>Синхронизация из Telegram</b>
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
