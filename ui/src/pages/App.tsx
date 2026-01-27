import React, { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store/app";
import { FileManager } from "../components/FileManager";
import { Login } from "../components/Login";

export default function App() {
  const { auth, setAuth, tree, refreshTree, error, setError } = useAppStore();

  useEffect(() => {
    (async () => {
      try {
        const status = await invoke<{ state: string }>("auth_status");
        setAuth(status.state);
        if (status.state === "ready") {
          await refreshTree();
        }
      } catch (e: any) {
        setError(String(e));
      }
    })();
  }, [refreshTree, setAuth, setError]);

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 16, maxWidth: 1100, margin: "0 auto" }}>
      <h1 style={{ marginBottom: 6 }}>CloudTG</h1>
      <p style={{ marginTop: 0, opacity: 0.8 }}>
        Файлы в Telegram, структура в SQLite. Сейчас Telegram слой в mock-режиме.
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
