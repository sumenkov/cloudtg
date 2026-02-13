import React, { useEffect, useState } from "react";
import { invokeSafe } from "../tauri";
import { useAppStore } from "../store/app";
import { Hint } from "./common/Hint";

const RECONCILE_SYNC_REQUIRED = "RECONCILE_SYNC_REQUIRED";

const panelStyle: React.CSSProperties = {
  padding: 14,
  border: "1px solid #ddd",
  borderRadius: 12,
  background: "#fff"
};

const mutedTextStyle: React.CSSProperties = {
  marginTop: 6,
  fontSize: 12,
  opacity: 0.72,
  lineHeight: 1.45
};

const inputStyle: React.CSSProperties = {
  width: "100%",
  maxWidth: "100%",
  minWidth: 0,
  boxSizing: "border-box",
  padding: 10,
  borderRadius: 10,
  border: "1px solid #ccc"
};

const buttonStyle: React.CSSProperties = {
  padding: "10px 12px",
  borderRadius: 10
};

const groupStyle: React.CSSProperties = {
  padding: 12,
  border: "1px solid #eee",
  borderRadius: 10,
  background: "#fcfcfc"
};

type SettingsProps = {
  onRequestLogout?: () => void;
  logoutBusy?: boolean;
  onRequestHelp?: () => void;
  helpBusy?: boolean;
  onRequestClose?: () => void;
};

export function Settings({
  onRequestLogout,
  logoutBusy = false,
  onRequestHelp,
  helpBusy = false,
  onRequestClose
}: SettingsProps) {
  const {
    auth,
    setError,
    refreshAuth,
    refreshSettings,
    refreshTree,
    tdlibBuild,
    tdlibLogs,
    tgSettings
  } = useAppStore();
  const creds = tgSettings.credentials;
  const [tdlibPath, setTdlibPath] = useState("");
  const [apiId, setApiId] = useState("");
  const [apiHash, setApiHash] = useState("");
  const [remember, setRemember] = useState(true);
  const [storageMode, setStorageMode] = useState<"keychain" | "encrypted">("keychain");
  const [encryptPassword, setEncryptPassword] = useState("");
  const [unlockPassword, setUnlockPassword] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [unlocking, setUnlocking] = useState(false);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [creating, setCreating] = useState(false);
  const [integrityStatus, setIntegrityStatus] = useState<string | null>(null);
  const [integrityBusy, setIntegrityBusy] = useState(false);
  const [integrityLimit, setIntegrityLimit] = useState("100");
  const [backupBusy, setBackupBusy] = useState(false);
  const [restoreBusy, setRestoreBusy] = useState(false);
  const [openBackupBusy, setOpenBackupBusy] = useState(false);
  const [backupStatus, setBackupStatus] = useState<string | null>(null);
  const [channelStatus, setChannelStatus] = useState<string | null>(null);
  const [saveWarning, setSaveWarning] = useState<string | null>(null);
  const [tdlibCacheMb, setTdlibCacheMb] = useState<number | null>(null);
  const [tdlibCacheStatus, setTdlibCacheStatus] = useState<string | null>(null);
  const [tdlibCacheRefreshing, setTdlibCacheRefreshing] = useState(false);
  const [tdlibCacheClearing, setTdlibCacheClearing] = useState(false);

  const buildState = tdlibBuild.state;
  const isBuilding =
    buildState === "start" ||
    buildState === "clone" ||
    buildState === "configure" ||
    buildState === "build" ||
    buildState === "download";
  const isError = buildState === "error";
  const isSuccess = buildState === "success";
  const showGperfHint = tdlibBuild.detail?.toLowerCase().includes("gperf");

  const sourceLabel =
    creds.source === "keychain"
      ? "системное хранилище"
      : creds.source === "encrypted"
        ? "зашифрованный файл"
        : creds.source === "runtime"
          ? "текущий запуск"
          : creds.source === "env"
            ? "переменные окружения"
            : "неизвестно";

  const stepKeysConfigured = creds.available || creds.locked;
  const stepTdlibReady = isSuccess || (!isBuilding && !isError);
  const stepAuthReady = auth === "ready";
  const keychainFallbackWarning =
    "Системное хранилище недоступно. Укажи пароль шифрования или выбери режим «Зашифрованный файл».";

  async function refreshTdlibCacheSize() {
    const cache = await invokeSafe<{ bytes: number; megabytes: number }>("tdlib_cache_size");
    setTdlibCacheMb(cache.megabytes);
  }

  useEffect(() => {
    (async () => {
      try {
        await refreshSettings();
        const s = useAppStore.getState().tgSettings;
        if (s.tdlib_path) setTdlibPath(s.tdlib_path);
      } catch (e: any) {
        setError(String(e));
      }
      try {
        await refreshTdlibCacheSize();
      } catch {
        setTdlibCacheStatus("Не удалось получить размер кеша TDLib.");
      }
    })();
  }, [refreshSettings, setError]);

  useEffect(() => {
    if (!remember || storageMode !== "keychain" || Boolean(encryptPassword.trim())) {
      setSaveWarning((prev) => (prev === keychainFallbackWarning ? null : prev));
    }
  }, [remember, storageMode, encryptPassword]);

  return (
    <div style={{ display: "grid", gap: 14, width: "100%" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10 }}>
        <b style={{ fontSize: 20 }}>Настройки</b>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", justifyContent: "flex-end" }}>
          {onRequestHelp ? (
            <button
              onClick={() => onRequestHelp()}
              disabled={helpBusy}
              style={{ ...buttonStyle, opacity: helpBusy ? 0.7 : 1, cursor: helpBusy ? "wait" : "pointer" }}
            >
              {helpBusy ? "Загружаю..." : "Справка"}
            </button>
          ) : null}
          {onRequestClose ? (
            <button
              onClick={() => onRequestClose()}
              disabled={logoutBusy}
              style={{ ...buttonStyle, opacity: logoutBusy ? 0.7 : 1, cursor: logoutBusy ? "wait" : "pointer" }}
            >
              Закрыть
            </button>
          ) : null}
          {auth === "ready" ? (
            <button
              onClick={() => onRequestLogout?.()}
              disabled={logoutBusy}
              style={{ ...buttonStyle, opacity: logoutBusy ? 0.7 : 1, cursor: logoutBusy ? "wait" : "pointer" }}
            >
              {logoutBusy ? "Выхожу..." : "Выйти из аккаунта"}
            </button>
          ) : null}
        </div>
      </div>

      {status ? (
        <div style={{ ...panelStyle, background: "#f7f7f7", padding: 12 }}>{status}</div>
      ) : null}

      <div style={{ display: "grid", gap: 12, gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))" }}>
        <div style={{ ...panelStyle, background: "#fafafa" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <b>Быстрый старт</b>
            <Hint text="Эти шаги помогают быстро понять, что уже готово для работы." />
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 8, fontSize: 13 }}>
            <div>{stepKeysConfigured ? "[x]" : "[ ]"} 1. Настроить API_ID / API_HASH</div>
            <div>{stepTdlibReady ? "[x]" : "[ ]"} 2. Подготовить TDLib</div>
            <div>{stepAuthReady ? "[x]" : "[ ]"} 3. Пройти авторизацию Telegram</div>
            <div>{stepAuthReady ? "[x]" : "[ ]"} 4. Обновить файлы из Telegram на главном экране</div>
          </div>
        </div>

        <div style={{ ...panelStyle, background: "#fafafa" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <b>Текущее состояние</b>
            <Hint text="Краткий статус ключей, TDLib и авторизации." />
          </div>
          <div style={{ marginTop: 10, display: "grid", gap: 8, fontSize: 13 }}>
            <div>Ключи: {creds.available ? `доступны (${sourceLabel})` : creds.locked ? "заблокированы" : "не заданы"}</div>
            <div>TDLib: {isError ? "ошибка" : isBuilding ? "подготовка" : stepTdlibReady ? "готово" : "ожидание"}</div>
            <div>Авторизация: {stepAuthReady ? "выполнена" : "не выполнена"}</div>
          </div>
        </div>
      </div>

      <div style={panelStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Подключение Telegram</b>
          <Hint text="API-ключи, хранение секретов и путь к TDLib." />
        </div>

        <div style={{ marginTop: 12, display: "grid", gap: 12 }}>
          <div style={groupStyle}>
            <div style={{ fontSize: 12, opacity: 0.72, marginBottom: 8 }}>Ключи Telegram API</div>
            <div style={{ display: "grid", gap: 10, gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))" }}>
              <label style={{ display: "grid", gap: 6, minWidth: 0 }}>
                API_ID
                <input
                  value={apiId}
                  onChange={(e) => setApiId(e.target.value)}
                  placeholder="123456"
                  style={inputStyle}
                />
              </label>
              <label style={{ display: "grid", gap: 6, minWidth: 0 }}>
                API_HASH
                <input
                  value={apiHash}
                  onChange={(e) => setApiHash(e.target.value)}
                  placeholder="abcdef..."
                  style={inputStyle}
                  type="password"
                />
              </label>
            </div>
          </div>

          <div style={groupStyle}>
            <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <input
                type="checkbox"
                checked={remember}
                onChange={(e) => setRemember(e.target.checked)}
              />
              Запомнить ключи
            </label>

            {remember ? (
              <div style={{ marginTop: 10, display: "grid", gap: 8 }}>
                <div style={{ fontSize: 12, opacity: 0.72 }}>Где хранить ключи:</div>
                <div style={{ display: "grid", gap: 8, gridTemplateColumns: "repeat(auto-fit, minmax(260px, 1fr))" }}>
                  <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
                    <input
                      type="radio"
                      name="storageMode"
                      checked={storageMode === "keychain"}
                      onChange={() => setStorageMode("keychain")}
                    />
                    Системное хранилище (рекомендуется)
                  </label>
                  <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
                    <input
                      type="radio"
                      name="storageMode"
                      checked={storageMode === "encrypted"}
                      onChange={() => setStorageMode("encrypted")}
                    />
                    Зашифрованный файл
                  </label>
                </div>
                <label style={{ display: "grid", gap: 6, minWidth: 0 }}>
                  {storageMode === "encrypted"
                    ? "Пароль для шифрования (обязательно)"
                    : "Пароль для шифрования (если keychain недоступен)"}
                  <input
                    value={encryptPassword}
                    onChange={(e) => setEncryptPassword(e.target.value)}
                    placeholder="пароль"
                    style={inputStyle}
                    type="password"
                  />
                </label>
              </div>
            ) : (
              <div style={mutedTextStyle}>Ключи будут использоваться только в текущем запуске.</div>
            )}

            {creds.available ? (
              <div style={mutedTextStyle}>Ключи доступны. Источник: {sourceLabel}.</div>
            ) : creds.locked ? (
              <div style={mutedTextStyle}>Ключи сохранены в зашифрованном виде. Нужен пароль для разблокировки.</div>
            ) : (
              <div style={mutedTextStyle}>Ключи еще не заданы.</div>
            )}
          </div>

          {creds.locked ? (
            <div style={groupStyle}>
              <b style={{ fontSize: 14 }}>Разблокировать ключи</b>
              <div style={{ marginTop: 8, display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
                <div style={{ flex: "1 1 220px", minWidth: 0 }}>
                  <input
                    value={unlockPassword}
                    onChange={(e) => setUnlockPassword(e.target.value)}
                    placeholder="пароль"
                    style={inputStyle}
                    type="password"
                  />
                </div>
                <button
                  onClick={async () => {
                    try {
                      setUnlocking(true);
                      setStatus("Разблокирую ключи...");
                      await invokeSafe("settings_unlock_tg", { password: unlockPassword });
                      setUnlockPassword("");
                      await refreshSettings();
                      await refreshAuth();
                      setError(null);
                      setStatus("Ключи разблокированы.");
                    } catch (e: any) {
                      setStatus("Не удалось разблокировать ключи");
                      setError(String(e));
                    } finally {
                      setUnlocking(false);
                    }
                  }}
                  disabled={unlocking}
                  style={{ ...buttonStyle, opacity: unlocking ? 0.7 : 1, cursor: unlocking ? "wait" : "pointer" }}
                >
                  {unlocking ? "Разблокирую..." : "Разблокировать"}
                </button>
              </div>
            </div>
          ) : null}

          <div style={groupStyle}>
            <label style={{ display: "grid", gap: 6, minWidth: 0 }}>
              Путь к TDLib (libtdjson)
              <div style={{ display: "flex", gap: 8, flexWrap: "wrap", alignItems: "center" }}>
                <div style={{ flex: "1 1 220px", minWidth: 0 }}>
                  <input
                    value={tdlibPath}
                    onChange={(e) => setTdlibPath(e.target.value)}
                    placeholder="/полный/путь/к/libtdjson.so"
                    style={inputStyle}
                  />
                </div>
                <button
                  onClick={async () => {
                    try {
                      setError(null);
                      const picked = await invokeSafe<string | null>("tdlib_pick");
                      if (picked) {
                        setTdlibPath(picked);
                      }
                    } catch (e: any) {
                      setError(String(e));
                    }
                  }}
                  style={{ ...buttonStyle, whiteSpace: "nowrap" }}
                >
                  Открыть
                </button>
              </div>
            </label>
            <div style={mutedTextStyle}>
              Можно оставить пустым: приложение попробует найти, скачать или собрать TDLib автоматически.
            </div>
          </div>

          <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
            {saveWarning ? (
              <div
                style={{
                  width: "100%",
                  padding: "10px 12px",
                  borderRadius: 10,
                  border: "1px solid #f1a3a3",
                  background: "#ffecec",
                  fontSize: 12,
                  color: "#9d1f1f"
                }}
              >
                {saveWarning}
              </div>
            ) : null}
            <button
              onClick={async () => {
                try {
                  setSaving(true);
                  setSaveWarning(null);
                  setStatus("Сохраняю...");
                  setError(null);
                  const apiIdValue = apiId.trim();
                  let apiIdNum: number | null = null;
                  if (apiIdValue) {
                    const parsed = Number.parseInt(apiIdValue, 10);
                    if (!Number.isFinite(parsed) || parsed <= 0) {
                      throw new Error("Некорректный API_ID");
                    }
                    apiIdNum = parsed;
                  }
                  if ((apiIdValue && !apiHash.trim()) || (!apiIdValue && apiHash.trim())) {
                    throw new Error("Нужно заполнить и API_ID, и API_HASH");
                  }
                  if (remember && storageMode === "encrypted" && !encryptPassword.trim()) {
                    throw new Error("Нужен пароль для шифрования");
                  }
                  const res = await invokeSafe<{ storage?: string | null; message: string }>("settings_set_tg", {
                    input: {
                      apiId: apiIdNum,
                      apiHash: apiHash.trim() ? apiHash.trim() : null,
                      remember,
                      storageMode,
                      password: remember ? (encryptPassword.trim() ? encryptPassword : null) : null,
                      tdlibPath: tdlibPath.trim() ? tdlibPath.trim() : null
                    }
                  });
                  await refreshSettings();
                  await refreshAuth();
                  setStatus(res.message || "Настройки сохранены.");
                  setSaveWarning(null);
                  setApiId("");
                  setApiHash("");
                  setEncryptPassword("");
                } catch (e: any) {
                  const message = String(e);
                  if (message.includes("Системное хранилище недоступно")) {
                    setSaveWarning(keychainFallbackWarning);
                    setStatus(null);
                  } else {
                    setStatus("Не удалось сохранить настройки");
                  }
                  setError(message);
                } finally {
                  setSaving(false);
                }
              }}
              disabled={saving}
              style={{ ...buttonStyle, opacity: saving ? 0.7 : 1, cursor: saving ? "wait" : "pointer" }}
            >
              {saving ? "Сохраняю..." : "Сохранить подключение"}
            </button>

            <button
              onClick={async () => {
                try {
                  setTesting(true);
                  setStatus("Отправляю тестовое сообщение...");
                  await invokeSafe("tg_test_message");
                  setStatus("Тестовое сообщение отправлено. Проверь канал CloudTG.");
                } catch (e: any) {
                  setStatus("Не удалось отправить тестовое сообщение");
                  setError(String(e));
                } finally {
                  setTesting(false);
                }
              }}
              disabled={testing}
              style={{ ...buttonStyle, opacity: testing ? 0.7 : 1, cursor: testing ? "wait" : "pointer" }}
            >
              {testing ? "Проверяю..." : "Проверить связь с Telegram"}
            </button>
          </div>
        </div>
      </div>

      <div style={panelStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Обслуживание хранилища</b>
          <Hint text="Проверка целостности отмечает поврежденные записи, бэкап сохраняет локальную базу в отдельный канал." />
        </div>

        <div style={{ marginTop: 12, display: "grid", gap: 12, gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))" }}>
          <div style={groupStyle}>
            <b style={{ fontSize: 14 }}>Проверка целостности последних сообщений</b>
            <div style={mutedTextStyle}>Проверяет последние сообщения в канале хранения и помечает битые записи.</div>
            <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
              <input
                value={integrityLimit}
                onChange={(e) => setIntegrityLimit(e.target.value)}
                placeholder="100"
                style={{ ...inputStyle, width: 120 }}
              />
              <button
                onClick={async () => {
                  try {
                    setIntegrityBusy(true);
                    setIntegrityStatus("Проверяю...");
                    const limitValue = Number.parseInt(integrityLimit.trim() || "100", 10);
                    if (!Number.isFinite(limitValue) || limitValue <= 0) {
                      throw new Error("Некорректный лимит сообщений");
                    }
                    try {
                      const res = await invokeSafe<{ message: string }>("tg_reconcile_recent", { limit: limitValue });
                      setIntegrityStatus(res.message || "Готово.");
                      await refreshTree();
                    } catch (e: any) {
                      const msg = String(e);
                      if (msg.includes(RECONCILE_SYNC_REQUIRED)) {
                        const ok = window.confirm(
                          "Импорт из канала хранения еще не запускался. Проверка может пропустить старые сообщения. Продолжить?"
                        );
                        if (!ok) {
                          setIntegrityStatus("Проверка отменена.");
                          return;
                        }
                        const res = await invokeSafe<{ message: string }>("tg_reconcile_recent", {
                          limit: limitValue,
                          force: true
                        });
                        setIntegrityStatus(res.message || "Готово.");
                        await refreshTree();
                      } else {
                        setIntegrityStatus("Не удалось выполнить проверку");
                        setError(msg);
                      }
                    }
                  } catch (e: any) {
                    setIntegrityStatus("Не удалось выполнить проверку");
                    setError(String(e));
                  } finally {
                    setIntegrityBusy(false);
                  }
                }}
                disabled={integrityBusy}
                style={{ ...buttonStyle, opacity: integrityBusy ? 0.7 : 1, cursor: integrityBusy ? "wait" : "pointer" }}
              >
                {integrityBusy ? "Проверяю..." : "Запустить проверку"}
              </button>
            </div>
            {integrityStatus ? <div style={{ marginTop: 8, fontSize: 12, opacity: 0.75 }}>{integrityStatus}</div> : null}
          </div>

          <div style={groupStyle}>
            <b style={{ fontSize: 14 }}>Бэкап базы</b>
            <div style={mutedTextStyle}>Бэкап сохраняется в отдельный канал <b>CloudTG Backups</b>.</div>
            <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
              <button
                onClick={async () => {
                  try {
                    setBackupBusy(true);
                    setBackupStatus("Создаю бэкап...");
                    const res = await invokeSafe<{ message: string }>("backup_create");
                    setBackupStatus(res.message || "Бэкап создан.");
                  } catch (e: any) {
                    setBackupStatus("Не удалось создать бэкап");
                    setError(String(e));
                  } finally {
                    setBackupBusy(false);
                  }
                }}
                disabled={backupBusy}
                style={{ ...buttonStyle, opacity: backupBusy ? 0.7 : 1, cursor: backupBusy ? "wait" : "pointer" }}
              >
                {backupBusy ? "Создаю..." : "Создать бэкап"}
              </button>
              <button
                onClick={async () => {
                  if (!window.confirm("Восстановить базу из последнего бэкапа? Потребуется перезапуск приложения.")) {
                    return;
                  }
                  try {
                    setRestoreBusy(true);
                    setBackupStatus("Подготавливаю восстановление...");
                    const res = await invokeSafe<{ message: string }>("backup_restore");
                    setBackupStatus(res.message || "Восстановление подготовлено. Перезапусти приложение.");
                  } catch (e: any) {
                    setBackupStatus("Не удалось подготовить восстановление");
                    setError(String(e));
                  } finally {
                    setRestoreBusy(false);
                  }
                }}
                disabled={restoreBusy}
                style={{
                  ...buttonStyle,
                  background: "#fef5e6",
                  border: "1px solid #f2c185",
                  opacity: restoreBusy ? 0.7 : 1,
                  cursor: restoreBusy ? "wait" : "pointer"
                }}
              >
                {restoreBusy ? "Подготавливаю..." : "Восстановить базу из бэкапа"}
              </button>
            </div>
            {backupStatus ? <div style={{ marginTop: 8, fontSize: 12, opacity: 0.75 }}>{backupStatus}</div> : null}
          </div>

          <div style={groupStyle}>
            <b style={{ fontSize: 14 }}>Каналы хранения</b>
            <div style={mutedTextStyle}>Открыть канал бэкапов и создать новый основной канал CloudTG.</div>
            <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
              <button
                onClick={async () => {
                  try {
                    setOpenBackupBusy(true);
                    setChannelStatus("Открываю канал бэкапов...");
                    const res = await invokeSafe<{ message: string }>("backup_open_channel");
                    setChannelStatus(res.message || "Канал открыт.");
                  } catch (e: any) {
                    setChannelStatus("Не удалось открыть канал");
                    setError(String(e));
                  } finally {
                    setOpenBackupBusy(false);
                  }
                }}
                disabled={openBackupBusy}
                style={{ ...buttonStyle, opacity: openBackupBusy ? 0.7 : 1, cursor: openBackupBusy ? "wait" : "pointer" }}
              >
                {openBackupBusy ? "Открываю..." : "Открыть канал бэкапов"}
              </button>
              <button
                onClick={async () => {
                  if (!window.confirm("Создать новый канал и перенести туда данные из базы?")) {
                    return;
                  }
                  try {
                    setCreating(true);
                    setChannelStatus("Создаю новый канал и переношу данные...");
                    await invokeSafe("tg_create_channel");
                    setChannelStatus("Канал создан. Данные перенесены. Проверь новый канал CloudTG.");
                  } catch (e: any) {
                    setChannelStatus("Не удалось создать новый канал");
                    setError(String(e));
                  } finally {
                    setCreating(false);
                  }
                }}
                disabled={creating}
                style={{
                  ...buttonStyle,
                  background: "#fee",
                  border: "1px solid #f99",
                  opacity: creating ? 0.7 : 1,
                  cursor: creating ? "wait" : "pointer"
                }}
              >
                {creating ? "Создаю..." : "Создать новый канал CloudTG"}
              </button>
            </div>
            {channelStatus ? <div style={{ marginTop: 8, fontSize: 12, opacity: 0.75 }}>{channelStatus}</div> : null}
          </div>

          <div style={groupStyle}>
            <b style={{ fontSize: 14 }}>Кеш TDLib</b>
            <div style={mutedTextStyle}>
              Оценка объема кеша: <b>{tdlibCacheMb === null ? "—" : `${tdlibCacheMb.toFixed(1)} МБ`}</b>
            </div>
            <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
              <button
                onClick={async () => {
                  try {
                    setTdlibCacheRefreshing(true);
                    setTdlibCacheStatus("Обновляю оценку кеша TDLib...");
                    await refreshTdlibCacheSize();
                    setTdlibCacheStatus("Оценка кеша TDLib обновлена.");
                  } catch (e: any) {
                    setTdlibCacheStatus("Не удалось обновить оценку кеша TDLib");
                    setError(String(e));
                  } finally {
                    setTdlibCacheRefreshing(false);
                  }
                }}
                disabled={tdlibCacheRefreshing || tdlibCacheClearing}
                style={{
                  ...buttonStyle,
                  opacity: tdlibCacheRefreshing || tdlibCacheClearing ? 0.7 : 1,
                  cursor: tdlibCacheRefreshing || tdlibCacheClearing ? "wait" : "pointer"
                }}
              >
                {tdlibCacheRefreshing ? "Обновляю..." : "Обновить оценку"}
              </button>
              <button
                onClick={async () => {
                  if (!window.confirm("Очистить кеш TDLib? Локальные файлы в папке downloads не будут удалены.")) {
                    return;
                  }
                  try {
                    setTdlibCacheClearing(true);
                    setTdlibCacheStatus("Очищаю кеш TDLib...");
                    const res = await invokeSafe<{
                      message: string;
                      before_bytes: number;
                      after_bytes: number;
                      freed_bytes: number;
                      failures: number;
                    }>("tdlib_cache_clear");
                    setTdlibCacheMb(res.after_bytes / (1024 * 1024));
                    setTdlibCacheStatus(res.message || "Кеш TDLib очищен.");
                  } catch (e: any) {
                    setTdlibCacheStatus("Не удалось очистить кеш TDLib");
                    setError(String(e));
                  } finally {
                    setTdlibCacheClearing(false);
                  }
                }}
                disabled={tdlibCacheRefreshing || tdlibCacheClearing}
                style={{
                  ...buttonStyle,
                  opacity: tdlibCacheRefreshing || tdlibCacheClearing ? 0.7 : 1,
                  cursor: tdlibCacheRefreshing || tdlibCacheClearing ? "wait" : "pointer"
                }}
              >
                {tdlibCacheClearing ? "Очищаю..." : "Очистить кеш TDLib"}
              </button>
            </div>
            {tdlibCacheStatus ? <div style={{ marginTop: 8, fontSize: 12, opacity: 0.75 }}>{tdlibCacheStatus}</div> : null}
          </div>
        </div>
      </div>

      {tdlibBuild.state && tdlibBuild.state !== "success" ? (
        <div
          style={{
            ...panelStyle,
            border: isError ? "1px solid #f99" : "1px solid #ddd",
            background: isError ? "#fee" : "#fafafa"
          }}
        >
          <b>Статус TDLib</b>
          <div style={{ marginTop: 6 }}>{tdlibBuild.message}</div>
          {isBuilding ? (
            <div style={mutedTextStyle}>Идет подготовка TDLib. Прогресс отображается на главном экране.</div>
          ) : null}
          {isSuccess && tdlibBuild.detail ? (
            <div style={mutedTextStyle}>Файл: {tdlibBuild.detail}</div>
          ) : null}
          {isError && tdlibBuild.detail ? (
            <pre style={{ marginTop: 8, whiteSpace: "pre-wrap", fontSize: 12 }}>{tdlibBuild.detail}</pre>
          ) : null}
          {isError && showGperfHint ? (
            <div style={{ marginTop: 8, fontSize: 12 }}>
              Подсказка: установи пакет <b>gperf</b> и снова нажми «Сохранить подключение».
            </div>
          ) : null}
          {tdlibLogs.length ? (
            <details style={{ marginTop: 8 }}>
              <summary style={{ cursor: "pointer" }}>Показать лог TDLib</summary>
              <pre style={{ marginTop: 10, whiteSpace: "pre-wrap", fontSize: 12, maxHeight: 220, overflow: "auto" }}>
                {tdlibLogs.map((l, i) => `[${l.stream}] ${l.line}`).join("\n")}
              </pre>
            </details>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
