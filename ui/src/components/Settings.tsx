import React, { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store/app";

type TgSettingsView = {
  api_id: number | null;
  api_hash: string | null;
};

export function Settings({ onClose }: { onClose?: () => void }) {
  const { setError, refreshAuth } = useAppStore();
  const [apiId, setApiId] = useState("");
  const [apiHash, setApiHash] = useState("");
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    (async () => {
      try {
        const s = await invoke<TgSettingsView>("settings_get_tg");
        if (s.api_id) setApiId(String(s.api_id));
        if (s.api_hash) setApiHash(s.api_hash);
      } catch (e: any) {
        setError(String(e));
      }
    })();
  }, [setError]);

  return (
    <div style={{ display: "grid", gap: 12, maxWidth: 520 }}>
      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <b>Настройки Telegram API</b>
        <div style={{ opacity: 0.8, marginTop: 6 }}>
          Укажи значения API_ID и API_HASH, полученные в Telegram.
        </div>
      </div>

      <label>
        API_ID
        <input
          value={apiId}
          onChange={(e) => setApiId(e.target.value)}
          placeholder="123456"
          style={{ width: "100%", padding: 10 }}
        />
      </label>

      <label>
        API_HASH
        <input
          value={apiHash}
          onChange={(e) => setApiHash(e.target.value)}
          placeholder="0123456789abcdef0123456789abcdef"
          style={{ width: "100%", padding: 10 }}
        />
      </label>

      <div style={{ display: "flex", gap: 10 }}>
        <button
          onClick={async () => {
            try {
              const id = parseInt(apiId.trim(), 10);
              if (!Number.isFinite(id) || id <= 0) {
                setStatus("API_ID должен быть положительным числом");
                return;
              }
              if (!apiHash.trim()) {
                setStatus("API_HASH не может быть пустым");
                return;
              }
              await invoke("settings_set_tg", { api_id: id, api_hash: apiHash.trim() });
              await refreshAuth();
              setStatus("Сохранено. Можно продолжить авторизацию.");
            } catch (e: any) {
              setError(String(e));
            }
          }}
          style={{ padding: 10, borderRadius: 10 }}
        >
          Сохранить
        </button>
        {onClose ? (
          <button onClick={onClose} style={{ padding: 10, borderRadius: 10, opacity: 0.8 }}>
            Закрыть
          </button>
        ) : null}
      </div>

      {status ? (
        <div style={{ padding: 10, borderRadius: 8, background: "#f6f6f6" }}>
          {status}
        </div>
      ) : null}
    </div>
  );
}
