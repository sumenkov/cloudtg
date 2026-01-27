import React, { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../store/app";
import { FileManager } from "../components/FileManager";
import { Login } from "../components/Login";
import { Settings } from "../components/Settings";

export default function App() {
  const { auth, setAuth, tree, refreshTree, error, setError, refreshAuth } = useAppStore();
  const [showSettings, setShowSettings] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | null = null;

    (async () => {
      try {
        const state = await refreshAuth();
        if (state === "ready") {
          await refreshTree();
        }

        unlisten = await listen<{ state: string }>("auth_state_changed", async (event) => {
          setAuth(event.payload.state);
          if (event.payload.state === "ready") {
            await refreshTree();
          }
        });
      } catch (e: any) {
        setError(String(e));
      }
    })();

    return () => {
      if (unlisten) {
        unlisten();
      }
    };
  }, [refreshAuth, refreshTree, setAuth, setError]);

  useEffect(() => {
    if (auth === "wait_config") {
      setShowSettings(true);
    }
  }, [auth]);

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 16, maxWidth: 1100, margin: "0 auto" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <h1 style={{ marginBottom: 6 }}>CloudTG</h1>
        <button onClick={() => setShowSettings((v) => !v)} style={{ padding: "8px 12px", borderRadius: 10 }}>
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
        <Settings onClose={() => setShowSettings(false)} />
      ) : auth !== "ready" ? (
        <Login />
      ) : (
        <FileManager tree={tree} />
      )}
    </div>
  );
}
