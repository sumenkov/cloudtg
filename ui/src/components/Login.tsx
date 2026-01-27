import React, { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "../store/app";

export function Login() {
  const { auth, setError, refreshAuth } = useAppStore();
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");

  const phase = auth === "wait_password" ? "password" : auth === "wait_code" ? "code" : "phone";

  return (
    <div style={{ display: "grid", gap: 10, maxWidth: 520 }}>
      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <b>Авторизация Telegram</b>
        <div style={{ opacity: 0.8, marginTop: 6 }}>
          Нужны значения CLOUDTG_TG_API_ID и CLOUDTG_TG_API_HASH в окружении.
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
                await refreshAuth();
              } catch (e: any) {
                setError(String(e));
              }
            }}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Получить код
          </button>
        </div>
      ) : phase === "code" ? (
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
                await refreshAuth();
              } catch (e: any) {
                setError(String(e));
              }
            }}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Войти
          </button>
        </div>
      ) : (
        <div style={{ display: "grid", gap: 8 }}>
          <label>
            Пароль 2FA
            <input
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="••••••••"
              type="password"
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <button
            onClick={async () => {
              try {
                await invoke("auth_submit_password", { password });
                await refreshAuth();
              } catch (e: any) {
                setError(String(e));
              }
            }}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Подтвердить
          </button>
        </div>
      )}

      {auth === "closed" ? (
        <div style={{ padding: 12, border: "1px solid #f99", borderRadius: 10, background: "#fee" }}>
          Сессия закрыта. Перезапусти приложение.
        </div>
      ) : null}
    </div>
  );
}
