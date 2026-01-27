import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store/app";

export function Login() {
  const { setAuth, setError } = useAppStore();
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [phase, setPhase] = useState<"phone" | "code">("phone");

  return (
    <div style={{ display: "grid", gap: 10, maxWidth: 520 }}>
      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <b>Авторизация Telegram</b>
        <div style={{ opacity: 0.8, marginTop: 6 }}>
          Сейчас backend работает в режиме mock. Команды оставлены под TDLib.
        </div>
      </div>

      {phase === "phone" ? (
        <div style={{ display: "grid", gap: 8 }}>
          <label>
            Телефон
            <input
              value={phone}
              onChange={(e) => setPhone(e.target.value)}
              placeholder="+49..."
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <button
            onClick={async () => {
              try {
                await invoke("auth_start", { phone });
                setPhase("code");
              } catch (e: any) {
                setError(String(e));
              }
            }}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Получить код
          </button>
        </div>
      ) : (
        <div style={{ display: "grid", gap: 8 }}>
          <label>
            Код
            <input
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="12345"
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <button
            onClick={async () => {
              try {
                await invoke("auth_submit_code", { code });
                setAuth("ready");
              } catch (e: any) {
                setError(String(e));
              }
            }}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Войти
          </button>
          <button onClick={() => setPhase("phone")} style={{ padding: 10, borderRadius: 10, opacity: 0.8 }}>
            Назад
          </button>
        </div>
      )}
    </div>
  );
}
