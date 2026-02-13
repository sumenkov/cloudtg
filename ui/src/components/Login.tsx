import React, { useEffect, useMemo, useState } from "react";
import { invokeSafe } from "../tauri";
import { useAppStore } from "../store/app";
import { Hint } from "./common/Hint";

const panelStyle: React.CSSProperties = {
  padding: 12,
  border: "1px solid #ddd",
  borderRadius: 10
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  padding: 10,
  borderRadius: 10,
  border: "1px solid #ccc",
  boxSizing: "border-box"
};

const buttonStyle: React.CSSProperties = {
  padding: 10,
  borderRadius: 10
};

const DEFAULT_CODE_RESEND_COOLDOWN_SEC = 61;

function formatCountdown(totalSec: number): string {
  const minutes = Math.floor(totalSec / 60);
  const seconds = totalSec % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

export function Login() {
  const { auth, setError, refreshAuth, refreshSettings, tdlibBuild, tgSettings } = useAppStore();
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [keysPassword, setKeysPassword] = useState("");
  const [unlockBusy, setUnlockBusy] = useState(false);
  const [unlockStatus, setUnlockStatus] = useState<string | null>(null);
  const [authBusy, setAuthBusy] = useState(false);
  const [codeRequestPending, setCodeRequestPending] = useState(false);
  const [codeRequestInfo, setCodeRequestInfo] = useState<string | null>(null);
  const [forcePhoneStep, setForcePhoneStep] = useState(false);
  const [codeRequestedAtMs, setCodeRequestedAtMs] = useState<number | null>(null);
  const [resendCooldownSec, setResendCooldownSec] = useState<number>(DEFAULT_CODE_RESEND_COOLDOWN_SEC);
  const [resendNowMs, setResendNowMs] = useState<number>(Date.now());

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
  const backendPhase = auth === "wait_password" ? "password" : auth === "wait_code" ? "code" : "phone";
  const waitingForCodeState =
    codeRequestPending && auth !== "wait_code" && auth !== "wait_password" && auth !== "ready";
  const phase = forcePhoneStep ? "phone" : backendPhase === "phone" && waitingForCodeState ? "code" : backendPhase;
  const phoneDisabled = showConfigHint || authBusy;
  const codeInputDisabled = showConfigHint;
  const codeSubmitDisabled = showConfigHint || authBusy || auth !== "wait_code";
  const passwordDisabled = showConfigHint || authBusy;
  const resendCooldownLeftSec = codeRequestedAtMs
    ? Math.max(0, resendCooldownSec - Math.floor((resendNowMs - codeRequestedAtMs) / 1000))
    : 0;
  const canResendCode =
    auth === "wait_code" && !showConfigHint && !authBusy && Boolean(phone.trim()) && resendCooldownLeftSec === 0;

  useEffect(() => {
    if (!codeRequestedAtMs || resendCooldownLeftSec <= 0) {
      return;
    }
    const timer = window.setInterval(() => {
      setResendNowMs(Date.now());
    }, 1000);
    return () => {
      window.clearInterval(timer);
    };
  }, [codeRequestedAtMs, resendCooldownLeftSec]);

  useEffect(() => {
    if (auth === "wait_code") {
      setForcePhoneStep(false);
      setCodeRequestPending(false);
      setCodeRequestInfo("Код отправлен. Введите его ниже.");
      return;
    }
    if (auth === "wait_password" || auth === "ready") {
      setForcePhoneStep(false);
      setCodeRequestPending(false);
      setCodeRequestInfo(null);
      return;
    }
    if (auth === "wait_phone" && !codeRequestPending) {
      setForcePhoneStep(false);
      setCodeRequestInfo(null);
      setCodeRequestedAtMs(null);
      setResendCooldownSec(DEFAULT_CODE_RESEND_COOLDOWN_SEC);
    }
  }, [auth, codeRequestPending]);

  useEffect(() => {
    if (auth !== "wait_code" || forcePhoneStep) {
      return;
    }
    let disposed = false;
    (async () => {
      const cooldownSec = await readResendCooldownFromBackend();
      if (disposed) return;
      const now = Date.now();
      setResendCooldownSec(cooldownSec);
      setCodeRequestedAtMs(now);
      setResendNowMs(now);
    })();
    return () => {
      disposed = true;
    };
  }, [auth, forcePhoneStep]);

  async function runAuthAction(action: () => Promise<void>): Promise<string | null> {
    try {
      setAuthBusy(true);
      setError(null);
      await action();
      return await refreshAuth();
    } catch (e: any) {
      setError(String(e));
      return null;
    } finally {
      setAuthBusy(false);
    }
  }

  async function readResendCooldownFromBackend(): Promise<number> {
    try {
      const timeout = await invokeSafe<number | null>("auth_code_resend_timeout");
      if (typeof timeout === "number" && Number.isFinite(timeout) && timeout > 0) {
        return timeout + 1;
      }
    } catch {
      // Keep default cooldown when backend timeout is unavailable.
    }
    return DEFAULT_CODE_RESEND_COOLDOWN_SEC;
  }

  async function requestCode(repeat = false): Promise<void> {
    const normalizedPhone = phone.trim();
    if (!normalizedPhone) {
      setError("Введите номер телефона");
      return;
    }

    setForcePhoneStep(false);
    setCodeRequestPending(true);
    setCodeRequestInfo(repeat ? "Повторный запрос отправлен. Ожидаем код..." : "Запрос отправлен. Ожидаем код в Telegram...");

    const nextState = await runAuthAction(async () => {
      if (repeat) {
        await invokeSafe("auth_resend_code");
        return;
      }
      await invokeSafe("auth_start", { phone: normalizedPhone });
    });

    if (nextState === null) {
      setCodeRequestPending(false);
      setCodeRequestInfo(null);
      setCodeRequestedAtMs(null);
      setResendCooldownSec(DEFAULT_CODE_RESEND_COOLDOWN_SEC);
      return;
    }

    const cooldownSec = await readResendCooldownFromBackend();
    const now = Date.now();
    setResendCooldownSec(cooldownSec);
    setCodeRequestedAtMs(now);
    setResendNowMs(now);
  }

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
        done: codeRequestPending || auth === "wait_code" || auth === "wait_password" || auth === "ready",
        label: "Запросить код",
        detail:
          auth === "wait_code" || auth === "wait_password" || auth === "ready"
            ? "Готово"
            : codeRequestPending
              ? "Запрос отправлен"
              : "Ожидается"
      },
      {
        done: auth === "ready",
        label: "Завершить вход",
        detail: auth === "ready" ? "Готово" : "Ожидается"
      }
    ],
    [hasSettings, locked, buildInProgress, buildError, auth, codeRequestPending]
  );

  return (
    <div style={{ display: "grid", gap: 10, maxWidth: 560 }}>
      <div style={panelStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Авторизация Telegram</b>
          <Hint text="Нужны API_ID/API_HASH и рабочая TDLib. Всё это настраивается кнопкой «Настройки» в правом верхнем углу." />
        </div>
        <div style={{ opacity: 0.8, marginTop: 6 }}>
          Процесс входа идет по шагам. Если что-то не готово, сначала открой «Настройки».
        </div>
      </div>

      <div style={{ ...panelStyle, background: "#fafafa" }}>
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
        <div style={panelStyle}>
          <b>Разблокировать ключи</b>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.75 }}>
            Пароль нужен только если ключи были сохранены в зашифрованном файле.
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 8, maxWidth: 420 }}>
            <label style={{ display: "grid", gap: 6 }}>
              <span>Пароль</span>
              <input
                value={keysPassword}
                onChange={(e) => setKeysPassword(e.target.value)}
                placeholder="пароль"
                type="password"
                style={inputStyle}
              />
            </label>
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
              style={{ ...buttonStyle, opacity: unlockBusy ? 0.7 : 1, cursor: unlockBusy ? "wait" : "pointer" }}
            >
              {unlockBusy ? "Разблокирую..." : "Разблокировать"}
            </button>
            {unlockStatus ? <div style={{ fontSize: 12, opacity: 0.75 }}>{unlockStatus}</div> : null}
          </div>
        </div>
      ) : null}

      {phase === "phone" ? (
        <div style={panelStyle}>
          <b>Шаг 1. Номер телефона</b>
          <div style={{ marginTop: 10, display: "grid", gap: 10, maxWidth: 420 }}>
            <label style={{ display: "grid", gap: 6 }}>
              <span>Телефон</span>
              <input
                value={phone}
                onChange={(e) => setPhone(e.target.value)}
                placeholder="+49..."
                disabled={phoneDisabled}
                style={inputStyle}
              />
          </label>
          <button
            onClick={async () => {
              await requestCode(false);
            }}
            disabled={phoneDisabled}
            style={{ ...buttonStyle, opacity: phoneDisabled ? 0.7 : 1, cursor: phoneDisabled ? "not-allowed" : "pointer" }}
          >
            {authBusy ? "Отправляю..." : "Получить код"}
          </button>
        </div>
        </div>
      ) : phase === "code" ? (
        <div style={panelStyle}>
          <b>Шаг 2. Код из Telegram</b>
          {waitingForCodeState ? (
            <div style={{ marginTop: 8, padding: 10, borderRadius: 8, border: "1px solid #f2c185", background: "#fff7ed", fontSize: 12 }}>
              {codeRequestInfo ?? "Запрос отправлен. Ждем ответ Telegram и подтверждение TDLib..."}
            </div>
          ) : codeRequestInfo ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.75 }}>{codeRequestInfo}</div>
          ) : null}
          <div style={{ marginTop: 10, display: "grid", gap: 10, maxWidth: 420 }}>
            <label style={{ display: "grid", gap: 6 }}>
              <span>Код</span>
              <input
                value={code}
                onChange={(e) => setCode(e.target.value)}
                placeholder="12345"
                disabled={codeInputDisabled}
                style={inputStyle}
              />
          </label>
          <button
            onClick={async () => {
              await runAuthAction(async () => {
                await invokeSafe("auth_submit_code", { code });
              });
            }}
            disabled={codeSubmitDisabled}
            style={{ ...buttonStyle, opacity: codeSubmitDisabled ? 0.7 : 1, cursor: codeSubmitDisabled ? "not-allowed" : "pointer" }}
          >
            {waitingForCodeState ? "Ждем код..." : authBusy ? "Проверяю код..." : "Войти"}
          </button>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button
              onClick={() => {
                setForcePhoneStep(true);
                setCodeRequestPending(false);
                setCodeRequestInfo(null);
                setCode("");
                setCodeRequestedAtMs(null);
                setResendCooldownSec(DEFAULT_CODE_RESEND_COOLDOWN_SEC);
                setResendNowMs(Date.now());
              }}
              disabled={authBusy}
              style={{ ...buttonStyle, opacity: authBusy ? 0.7 : 1, cursor: authBusy ? "not-allowed" : "pointer" }}
            >
              Отмена
            </button>
            <button
              onClick={async () => {
                await requestCode(true);
              }}
              disabled={!canResendCode}
              style={{ ...buttonStyle, opacity: canResendCode ? 1 : 0.7, cursor: canResendCode ? "pointer" : "not-allowed" }}
            >
              {resendCooldownLeftSec > 0 ? `Повторить через ${formatCountdown(resendCooldownLeftSec)}` : "Повторить отправку"}
            </button>
          </div>
        </div>
        </div>
      ) : (
        <div style={panelStyle}>
          <b>Шаг 3. Пароль 2FA</b>
          <div style={{ marginTop: 10, display: "grid", gap: 10, maxWidth: 420 }}>
            <label style={{ display: "grid", gap: 6 }}>
              <span>Пароль 2FA</span>
              <input
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                placeholder="••••••••"
                type="password"
                disabled={passwordDisabled}
                style={inputStyle}
              />
          </label>
          <button
            onClick={async () => {
              await runAuthAction(async () => {
                await invokeSafe("auth_submit_password", { password });
              });
            }}
            disabled={passwordDisabled}
            style={{ ...buttonStyle, opacity: passwordDisabled ? 0.7 : 1, cursor: passwordDisabled ? "not-allowed" : "pointer" }}
          >
            {authBusy ? "Подтверждаю..." : "Подтвердить"}
          </button>
        </div>
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
