import React, { useEffect, useState } from "react";
import { listenSafe } from "../tauri";
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
    tdlibBuild
  } = useAppStore();
  const [showSettings, setShowSettings] = useState(false);
  const [autoShowSettings, setAutoShowSettings] = useState(true);
  const progressValue =
    tdlibBuild.progress === null ? null : Math.max(0, Math.min(100, tdlibBuild.progress));

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    (async () => {
      try {
        const state = await refreshAuth();
        await refreshSettings();
        await refreshTree();

        unlisteners.push(
          await listenSafe<{ state: string }>("auth_state_changed", async (event) => {
            setAuth(event.payload.state);
            if (event.payload.state === "ready") {
              await refreshTree();
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
    touchTdlibBuildOnLog
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
