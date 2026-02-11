import React, { useEffect, useState } from "react";
import { invokeSafe } from "../tauri";
import { useAppStore } from "../store/app";
import { Hint } from "./common/Hint";

const RECONCILE_SYNC_REQUIRED = "RECONCILE_SYNC_REQUIRED";

export function Settings({ onClose }: { onClose?: () => void }) {
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

  useEffect(() => {
    (async () => {
      try {
        await refreshSettings();
        const s = useAppStore.getState().tgSettings;
        if (s.tdlib_path) setTdlibPath(s.tdlib_path);
      } catch (e: any) {
        setError(String(e));
      }
    })();
  }, [refreshSettings, setError]);

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

  return (
    <div style={{ display: "grid", gap: 12, maxWidth: 760 }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 10 }}>
        <b style={{ fontSize: 18 }}>Настройки</b>
        {onClose ? (
          <button onClick={onClose} style={{ padding: 10, borderRadius: 10, opacity: 0.85 }}>
            Закрыть
          </button>
        ) : null}
      </div>

      {status ? (
        <div style={{ padding: 10, borderRadius: 8, background: "#f6f6f6" }}>
          {status}
        </div>
      ) : null}

      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10, background: "#fafafa" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Быстрый старт</b>
          <Hint text="Эти шаги помогают быстро понять, что уже готово для работы." />
        </div>
        <div style={{ marginTop: 8, display: "grid", gap: 6, fontSize: 13 }}>
          <div>{stepKeysConfigured ? "[x]" : "[ ]"} 1. Настроить API_ID / API_HASH</div>
          <div>{stepTdlibReady ? "[x]" : "[ ]"} 2. Подготовить TDLib</div>
          <div>{stepAuthReady ? "[x]" : "[ ]"} 3. Пройти авторизацию Telegram</div>
          <div>{stepAuthReady ? "[x]" : "[ ]"} 4. Обновить файлы из Telegram на главном экране</div>
        </div>
      </div>

      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Подключение Telegram</b>
          <Hint text="Здесь задаются ключи Telegram API и путь к TDLib." />
        </div>

        <div style={{ marginTop: 10, display: "grid", gap: 8 }}>
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
              placeholder="abcdef..."
              style={{ width: "100%", padding: 10 }}
              type="password"
            />
          </label>

          <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <input
              type="checkbox"
              checked={remember}
              onChange={(e) => setRemember(e.target.checked)}
            />
            Запомнить ключи
          </label>

          {remember ? (
            <div style={{ display: "grid", gap: 8, padding: 10, border: "1px solid #eee", borderRadius: 8 }}>
              <div style={{ fontSize: 12, opacity: 0.7 }}>Где хранить ключи:</div>
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
              <label>
                {storageMode === "encrypted"
                  ? "Пароль для шифрования (обязательно)"
                  : "Пароль для шифрования (если keychain недоступен)"}
                <input
                  value={encryptPassword}
                  onChange={(e) => setEncryptPassword(e.target.value)}
                  placeholder="пароль"
                  style={{ width: "100%", padding: 10 }}
                  type="password"
                />
              </label>
            </div>
          ) : (
            <div style={{ fontSize: 12, opacity: 0.7 }}>
              Ключи будут использоваться только в текущем запуске.
            </div>
          )}

          {!creds.keychain_available && remember && storageMode === "keychain" ? (
            <div style={{ fontSize: 12, color: "#b04a00" }}>
              Системное хранилище недоступно. Выбери режим «Зашифрованный файл» и задай пароль.
            </div>
          ) : null}

          {creds.available ? (
            <div style={{ fontSize: 12, opacity: 0.7 }}>
              Ключи доступны. Источник: {sourceLabel}.
            </div>
          ) : creds.locked ? (
            <div style={{ fontSize: 12, opacity: 0.7 }}>
              Ключи сохранены в зашифрованном виде. Нужен пароль для разблокировки.
            </div>
          ) : (
            <div style={{ fontSize: 12, opacity: 0.7 }}>Ключи еще не заданы.</div>
          )}
        </div>

        {creds.locked ? (
          <div style={{ marginTop: 10, paddingTop: 10, borderTop: "1px solid #eee", display: "grid", gap: 8 }}>
            <b>Разблокировать ключи</b>
            <input
              value={unlockPassword}
              onChange={(e) => setUnlockPassword(e.target.value)}
              placeholder="пароль"
              style={{ width: "100%", padding: 10 }}
              type="password"
            />
            <button
              onClick={async () => {
                try {
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
                }
              }}
              style={{ padding: 10, borderRadius: 10 }}
            >
              Разблокировать
            </button>
          </div>
        ) : null}

        <div style={{ marginTop: 10 }}>
          <label>
            Путь к TDLib (libtdjson)
            <input
              value={tdlibPath}
              onChange={(e) => setTdlibPath(e.target.value)}
              placeholder="/полный/путь/к/libtdjson.so"
              style={{ width: "100%", padding: 10 }}
            />
          </label>
          <div style={{ opacity: 0.7, marginTop: 4, fontSize: 12 }}>
            Можно оставить пустым: приложение попробует найти, скачать или собрать TDLib автоматически.
          </div>
        </div>

        <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap" }}>
          <button
            onClick={async () => {
              try {
                setSaving(true);
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
                if (remember && storageMode === "keychain" && !creds.keychain_available && !encryptPassword.trim()) {
                  throw new Error(
                    "Системное хранилище недоступно. Укажи пароль для шифрования или выбери режим «Зашифрованный файл»."
                  );
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
                setApiId("");
                setApiHash("");
                setEncryptPassword("");
              } catch (e: any) {
                setStatus("Не удалось сохранить настройки");
                setError(String(e));
              } finally {
                setSaving(false);
              }
            }}
            disabled={saving}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Сохранить подключение
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
            style={{ padding: 10, borderRadius: 10, opacity: testing ? 0.6 : 1 }}
          >
            Проверить связь с Telegram
          </button>
        </div>
      </div>

      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b>Обслуживание хранилища</b>
          <Hint text="Проверка целостности отмечает поврежденные записи, бэкап сохраняет локальную базу в отдельный канал." />
        </div>

        <div style={{ marginTop: 10, padding: 10, border: "1px solid #eee", borderRadius: 8 }}>
          <b>Проверка целостности последних сообщений</b>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
            Проверяет последние сообщения в канале хранения и помечает битые записи.
          </div>
          <div style={{ marginTop: 8, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
            <input
              value={integrityLimit}
              onChange={(e) => setIntegrityLimit(e.target.value)}
              placeholder="100"
              style={{ width: 120, padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
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
              style={{ padding: 10, borderRadius: 10 }}
            >
              Запустить проверку
            </button>
            {integrityStatus ? <div style={{ fontSize: 12, opacity: 0.75 }}>{integrityStatus}</div> : null}
          </div>
        </div>

        <div style={{ marginTop: 10, padding: 10, border: "1px solid #eee", borderRadius: 8 }}>
          <b>Бэкап базы</b>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
            Бэкап сохраняется в отдельный канал <b>CloudTG Backups</b>.
          </div>
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
              style={{ padding: 10, borderRadius: 10 }}
            >
              Создать бэкап
            </button>
            <button
              onClick={async () => {
                try {
                  setOpenBackupBusy(true);
                  setBackupStatus("Открываю канал бэкапов...");
                  const res = await invokeSafe<{ message: string }>("backup_open_channel");
                  setBackupStatus(res.message || "Канал открыт.");
                } catch (e: any) {
                  setBackupStatus("Не удалось открыть канал");
                  setError(String(e));
                } finally {
                  setOpenBackupBusy(false);
                }
              }}
              disabled={openBackupBusy}
              style={{ padding: 10, borderRadius: 10 }}
            >
              Открыть канал бэкапов
            </button>
            {backupStatus ? <div style={{ fontSize: 12, opacity: 0.75 }}>{backupStatus}</div> : null}
          </div>
        </div>
      </div>

      <div style={{ padding: 12, border: "1px solid #f3bcbc", borderRadius: 10, background: "#fff4f4" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
          <b style={{ color: "#9d1f1f" }}>Опасные действия</b>
          <Hint text="Эти операции могут изменить структуру хранения или потребовать перезапуск приложения." />
        </div>
        <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>
          Используй только если понимаешь последствия.
        </div>
        <div style={{ marginTop: 10, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
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
            style={{ padding: 10, borderRadius: 10, background: "#fef5e6", border: "1px solid #f2c185" }}
          >
            Восстановить базу из бэкапа
          </button>

          <button
            onClick={async () => {
              if (!window.confirm("Создать новый канал и перенести туда данные из базы?")) {
                return;
              }
              try {
                setCreating(true);
                setStatus("Создаю новый канал и переношу данные...");
                await invokeSafe("tg_create_channel");
                setStatus("Канал создан. Данные перенесены. Проверь новый канал CloudTG.");
              } catch (e: any) {
                setStatus("Не удалось создать новый канал");
                setError(String(e));
              } finally {
                setCreating(false);
              }
            }}
            disabled={creating}
            style={{ padding: 10, borderRadius: 10, background: "#fee", border: "1px solid #f99" }}
          >
            Создать новый канал CloudTG
          </button>
        </div>
      </div>

      {tdlibBuild.state && tdlibBuild.state !== "success" ? (
        <div
          style={{
            padding: 12,
            borderRadius: 10,
            border: isError ? "1px solid #f99" : "1px solid #ddd",
            background: isError ? "#fee" : "#fafafa"
          }}
        >
          <b>Статус TDLib</b>
          <div style={{ marginTop: 6 }}>{tdlibBuild.message}</div>
          {isBuilding ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Идет подготовка TDLib. Прогресс отображается на главном экране.
            </div>
          ) : null}
          {isSuccess && tdlibBuild.detail ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>Файл: {tdlibBuild.detail}</div>
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
