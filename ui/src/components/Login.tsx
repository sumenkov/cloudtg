import React, { useMemo, useState } from "react";
import { invokeSafe } from "../tauri";
import { useAppStore } from "../store/app";
import { Hint } from "./common/Hint";

export function Login() {
  const { auth, setError, refreshAuth, refreshSettings, tdlibBuild, tgSettings } = useAppStore();
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [keysPassword, setKeysPassword] = useState("");
  const [unlockBusy, setUnlockBusy] = useState(false);
  const [unlockStatus, setUnlockStatus] = useState<string | null>(null);

  const phase = auth === "wait_password" ? "password" : auth === "wait_code" ? "code" : "phone";
  const buildError = tdlibBuild.state === "error";
  const buildInProgress =
    tdlibBuild.state === "start" ||
    tdlibBuild.state === "clone" ||
    tdlibBuild.state === "configure" ||
    tdlibBuild.state === "build" ||
    tdlibBuild.state === "download";

  const creds = tgSettings.credentials;
  const hasSettings = creds.available;
  const locked = creds.locked;
  const showConfigHint = auth === "wait_config" || buildInProgress || buildError || locked || !hasSettings;
  const disabled = showConfigHint;

  const checklist = useMemo(
    () => [
      {
        done: hasSettings || locked,
        label: "Указать API_ID и API_HASH",
        detail: hasSettings ? "Ключи готовы" : locked ? "Ключи сохранены, нужен пароль" : "Нужна настройка"
      },
      {
        done: !buildInProgress && !buildError,
        label: "Подготовить TDLib",
        detail: buildInProgress ? "Идет подготовка" : buildError ? "Ошибка подготовки" : "Готово"
      },
      {
        done: auth === "wait_code" || auth === "wait_password" || auth === "ready",
        label: "Запросить код",
        detail: auth === "wait_phone" || auth === "wait_config" ? "Ожидается" : "Готово"
      },
      {
        done: auth === "ready",
        label: "Завершить вход",
        detail: auth === "ready" ? "Готово" : "Ожидается"
      }
    ],
    [hasSettings, locked, buildInProgress, buildError, auth]
  );

  return (
    <div style={{ display: "grid", gap: 10, maxWidth: 560 }}>
      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Авторизация Telegram</b>
          <Hint text="Нужны API_ID/API_HASH и рабочая TDLib. Всё это настраивается кнопкой «Настройки» в правом верхнем углу." />
        </div>
        <div style={{ opacity: 0.8, marginTop: 6 }}>
          Процесс входа идет по шагам. Если что-то не готово, сначала открой «Настройки».
        </div>
      </div>

      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10, background: "#fafafa" }}>
        <b>Чеклист первого запуска</b>
        <div style={{ marginTop: 8, display: "grid", gap: 6, fontSize: 13 }}>
          {checklist.map((item) => (
            <div key={item.label}>
              {item.done ? "[x]" : "[ ]"} {item.label}
              <span style={{ opacity: 0.65 }}> — {item.detail}</span>
            </div>
          ))}
        </div>
      </div>

      {showConfigHint ? (
        <div style={{ padding: 12, border: "1px solid #f99", borderRadius: 10, background: "#fee" }}>
          {buildInProgress ? (
            <div>Идет подготовка TDLib. Дождись завершения и продолжай вход.</div>
          ) : buildError ? (
            <div>Ошибка подготовки TDLib. Открой «Настройки», исправь и сохрани.</div>
          ) : locked ? (
            <div>Ключи зашифрованы. Введи пароль ниже, чтобы разблокировать.</div>
          ) : hasSettings ? (
            <div>Ключи сохранены. Если вход не стартует, проверь статус TDLib в «Настройках».</div>
          ) : (
            <div>Сначала укажи API_ID и API_HASH в «Настройках».</div>
          )}
        </div>
      ) : null}

      {locked ? (
        <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
          <b>Разблокировать ключи</b>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.75 }}>
            Пароль нужен только если ключи были сохранены в зашифрованном файле.
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 8 }}>
            <input
              value={keysPassword}
              onChange={(e) => setKeysPassword(e.target.value)}
              placeholder="пароль"
              type="password"
              style={{ width: "100%", padding: 10 }}
            />
            <button
              onClick={async () => {
                try {
                  setUnlockBusy(true);
                  setUnlockStatus("Разблокирую ключи...");
                  await invokeSafe("settings_unlock_tg", { password: keysPassword });
                  setKeysPassword("");
                  await refreshSettings();
                  await refreshAuth();
                  setError(null);
                  setUnlockStatus("Ключи разблокированы.");
                } catch (e: any) {
                  setUnlockStatus("Не удалось разблокировать ключи");
                  setError(String(e));
                } finally {
                  setUnlockBusy(false);
                }
              }}
              disabled={unlockBusy}
              style={{ padding: 10, borderRadius: 10, opacity: unlockBusy ? 0.7 : 1 }}
            >
              Разблокировать
            </button>
            {unlockStatus ? <div style={{ fontSize: 12, opacity: 0.75 }}>{unlockStatus}</div> : null}
          </div>
        </div>
      ) : null}

      {phase === "phone" ? (
        <div style={{ display: "grid", gap: 8 }}>
          <label>
            Телефон
            <input
              value={phone}
              onChange={(e) => setPhone(e.target.value)}
              placeholder="+49..."
              disabled={disabled}
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <button
            onClick={async () => {
              try {
                await invokeSafe("auth_start", { phone });
                await refreshAuth();
                setError(null);
              } catch (e: any) {
                setError(String(e));
              }
            }}
            disabled={disabled}
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
              disabled={disabled}
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <button
            onClick={async () => {
              try {
                await invokeSafe("auth_submit_code", { code });
                await refreshAuth();
                setError(null);
              } catch (e: any) {
                setError(String(e));
              }
            }}
            disabled={disabled}
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
              disabled={disabled}
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <button
            onClick={async () => {
              try {
                await invokeSafe("auth_submit_password", { password });
                await refreshAuth();
                setError(null);
              } catch (e: any) {
                setError(String(e));
              }
            }}
            disabled={disabled}
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
