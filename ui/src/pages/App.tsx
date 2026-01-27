import React, { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useAppStore } from "../store/app";
import { FileManager } from "../components/FileManager";
import { Login } from "../components/Login";

export default function App() {
  const { auth, setAuth, tree, refreshTree, error, setError, refreshAuth } = useAppStore();

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

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 16, maxWidth: 1100, margin: "0 auto" }}>
      <h1 style={{ marginBottom: 6 }}>CloudTG</h1>
      <p style={{ marginTop: 0, opacity: 0.8 }}>
        Файлы в Telegram, структура в SQLite. Авторизация через TDLib.
      </p>

      {error ? (
        <div style={{ background: "#fee", border: "1px solid #f99", padding: 12, borderRadius: 8 }}>
          <b>Ошибка:</b> {error}
        </div>
      ) : null}

      {auth !== "ready" ? <Login /> : <FileManager tree={tree} />}
    </div>
  );
}
