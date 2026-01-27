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
    pushTdlibLog
  } = useAppStore();
    useAppStore();
  const [showSettings, setShowSettings] = useState(false);
  const [autoShowSettings, setAutoShowSettings] = useState(true);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];

    (async () => {
      try {
        const state = await refreshAuth();
        await refreshSettings();
        if (state === "ready") {
          await refreshTree();
        }

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
              detail: event.payload.detail ?? null
            });
          })
        );

        unlisteners.push(
          await listenSafe<{ stream: "stdout" | "stderr"; line: string }>("tdlib_build_log", async (event) => {
            pushTdlibLog(event.payload.stream, event.payload.line);
          })
        );
      } catch (e: any) {
        setError(String(e));
      }
    })();

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [refreshAuth, refreshSettings, refreshTree, setAuth, setError, setTdlibBuild, clearTdlibLogs, pushTdlibLog]);

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
