import React, { useEffect, useState } from "react";
import { invokeSafe } from "../tauri";
import { useAppStore } from "../store/app";

type TgSettingsView = {
  tdlib_path: string | null;
  credentials: {
    available: boolean;
    source: string | null;
    keychain_available: boolean;
    encrypted_present: boolean;
    locked: boolean;
  };
};

const RECONCILE_SYNC_REQUIRED = "RECONCILE_SYNC_REQUIRED";

export function Settings({ onClose }: { onClose?: () => void }) {
  const { setError, refreshAuth, refreshSettings, refreshTree, tdlibBuild, tdlibLogs, tgSettings } = useAppStore();
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
  const [reconcileStatus, setReconcileStatus] = useState<string | null>(null);
  const [reconcileBusy, setReconcileBusy] = useState(false);
  const [reconcileLimit, setReconcileLimit] = useState("100");
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
        const s = await invokeSafe<TgSettingsView>("settings_get_tg");
        if (s.tdlib_path) setTdlibPath(s.tdlib_path);
      } catch (e: any) {
        setError(String(e));
      }
    })();
  }, [setError]);

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

  return (
    <div style={{ display: "grid", gap: 12, maxWidth: 520 }}>
      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <b>Настройки Telegram API</b>
        <div style={{ opacity: 0.8, marginTop: 6 }}>
          Ключи можно сохранить в системном хранилище или использовать только для текущего запуска.
          Если системное хранилище недоступно, ключи можно сохранить в зашифрованном файле по паролю.
        </div>
      </div>

      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <b>Ключи Telegram</b>
        <div style={{ marginTop: 8, display: "grid", gap: 8 }}>
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
            <div style={{ display: "grid", gap: 8 }}>
              <div style={{ fontSize: 12, opacity: 0.7 }}>Где хранить ключи:</div>
              <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input
                  type="radio"
                  name="storageMode"
                  checked={storageMode === "keychain"}
                  onChange={() => setStorageMode("keychain")}
                />
                Системное хранилище (по умолчанию)
              </label>
              <label style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <input
                  type="radio"
                  name="storageMode"
                  checked={storageMode === "encrypted"}
                  onChange={() => setStorageMode("encrypted")}
                />
                Зашифрованный файл (принудительно)
              </label>
              <label>
                {storageMode === "encrypted"
                  ? "Пароль для шифрования (обязательно)"
                  : "Пароль для шифрования (если системное хранилище недоступно)"}
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
              Если Системное хранилище ключей недоступно. Можно сохранить ключи в зашифрованном файле с паролем.
            </div>
          ) : null}
          {creds.available ? (
            <div style={{ fontSize: 12, opacity: 0.7 }}>
              Ключи доступны. Источник: {sourceLabel}.
            </div>
          ) : creds.locked ? (
            <div style={{ fontSize: 12, opacity: 0.7 }}>
              Ключи сохранены, но зашифрованы. Нужен пароль для расшифровки.
            </div>
          ) : (
            <div style={{ fontSize: 12, opacity: 0.7 }}>
              Ключи не заданы.
            </div>
          )}
        </div>
      </div>

      {creds.locked ? (
        <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
          <b>Разблокировка ключей</b>
          <div style={{ marginTop: 8, display: "grid", gap: 8 }}>
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
        </div>
      ) : null}

      <label>
        Путь к TDLib (libtdjson)
        <input
          value={tdlibPath}
          onChange={(e) => setTdlibPath(e.target.value)}
          placeholder="/полный/путь/к/libtdjson.so"
          style={{ width: "100%", padding: 10 }}
        />
        <div style={{ opacity: 0.7, marginTop: 4 }}>
          Можно оставить пустым, если библиотека лежит рядом с бинарём приложения.
        </div>
      </label>

      <div style={{ padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
        <b>Реконсайл последних {reconcileLimit || "100"} сообщений</b>
        <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
          Проверяет последние сообщения в канале хранения и помечает битые записи. Если папка старая — увеличь лимит.
        </div>
        <div style={{ marginTop: 8, display: "flex", gap: 10, flexWrap: "wrap", alignItems: "center" }}>
          <input
            value={reconcileLimit}
            onChange={(e) => setReconcileLimit(e.target.value)}
            placeholder="100"
            style={{ width: 120, padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
          />
          <button
            onClick={async () => {
              try {
                setReconcileBusy(true);
                setReconcileStatus("Проверяю...");
                const limitValue = Number.parseInt(reconcileLimit.trim() || "100", 10);
                if (!Number.isFinite(limitValue) || limitValue <= 0) {
                  throw new Error("Некорректный лимит сообщений");
                }
                try {
                  const res = await invokeSafe<{ message: string }>("tg_reconcile_recent", { limit: limitValue });
                  setReconcileStatus(res.message || "Готово.");
                  await refreshTree();
                } catch (e: any) {
                  const msg = String(e);
                  if (msg.includes(RECONCILE_SYNC_REQUIRED)) {
                    const ok = window.confirm(
                      "Импорт из канала хранения ещё не запускался. Реконсайл может пропустить старые сообщения. Продолжить?"
                    );
                    if (!ok) {
                      setReconcileStatus("Реконсайл отменен.");
                      return;
                    }
                    const res = await invokeSafe<{ message: string }>("tg_reconcile_recent", { limit: limitValue, force: true });
                    setReconcileStatus(res.message || "Готово.");
                    await refreshTree();
                  } else {
                    setReconcileStatus("Не удалось выполнить реконсайл");
                    setError(msg);
                  }
                }
              } catch (e: any) {
                setReconcileStatus("Не удалось выполнить реконсайл");
                setError(String(e));
              } finally {
                setReconcileBusy(false);
              }
            }}
            disabled={reconcileBusy}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Запустить проверку
          </button>
          {reconcileStatus ? <div style={{ fontSize: 12, opacity: 0.7 }}>{reconcileStatus}</div> : null}
        </div>
      </div>

      <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
        <button
          onClick={async () => {
            try {
              setSaving(true);
              setStatus("Сохраняю...");
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
              setStatus(res.message || "Сохранено. Можно продолжить авторизацию.");
              setApiId("");
              setApiHash("");
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
          Сохранить
        </button>
        {onClose ? (
          <button onClick={onClose} style={{ padding: 10, borderRadius: 10, opacity: 0.8 }}>
            Закрыть
          </button>
        ) : null}
      </div>

      <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
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
          style={{ padding: 10, borderRadius: 10, opacity: creating ? 0.6 : 1 }}
        >
          Создать канал в Telegram
        </button>
      </div>

      {status ? (
        <div style={{ padding: 10, borderRadius: 8, background: "#f6f6f6" }}>
          {status}
        </div>
      ) : null}

      {tdlibBuild.state && tdlibBuild.state !== "success" ? (
        <div
          style={{
            padding: 12,
            borderRadius: 10,
            border: isError ? "1px solid #f99" : "1px solid #ddd",
            background: isError ? "#fee" : "#fafafa"
          }}
        >
          <b>Сборка TDLib</b>
          <div style={{ marginTop: 6 }}>{tdlibBuild.message}</div>
          {isBuilding ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Идет подготовка TDLib. Прогресс отображается в главном окне.
            </div>
          ) : null}
          {isSuccess && tdlibBuild.detail ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Файл: {tdlibBuild.detail}
            </div>
          ) : null}
          {isError && tdlibBuild.detail ? (
            <pre style={{ marginTop: 8, whiteSpace: "pre-wrap", fontSize: 12 }}>{tdlibBuild.detail}</pre>
          ) : null}
          {isError && showGperfHint ? (
            <div style={{ marginTop: 8, fontSize: 12 }}>
              Подсказка: установи пакет <b>gperf</b> и запусти сборку снова.
            </div>
          ) : null}
          {isError ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Чтобы повторить сборку, снова нажми «Сохранить».
            </div>
          ) : null}
          {tdlibLogs.length ? (
            <pre style={{ marginTop: 10, whiteSpace: "pre-wrap", fontSize: 12, maxHeight: 220, overflow: "auto" }}>
              {tdlibLogs.map((l, i) => `[${l.stream}] ${l.line}`).join("\n")}
            </pre>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
