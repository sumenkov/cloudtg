import React, { useEffect, useState } from "react";
import { invokeSafe } from "../tauri";
import { useAppStore } from "../store/app";

type TgSettingsView = {
  api_id: number | null;
  api_hash: string | null;
  tdlib_path: string | null;
};

export function Settings({ onClose }: { onClose?: () => void }) {
  const { setError, refreshAuth, refreshSettings, tdlibBuild, tdlibLogs } = useAppStore();
  const [apiId, setApiId] = useState("");
  const [apiHash, setApiHash] = useState("");
  const [tdlibPath, setTdlibPath] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [creating, setCreating] = useState(false);
  const buildState = tdlibBuild.state;
  const isBuilding = buildState === "start" || buildState === "clone" || buildState === "configure" || buildState === "build";
  const isError = buildState === "error";
  const isSuccess = buildState === "success";
  const showGperfHint = tdlibBuild.detail?.toLowerCase().includes("gperf");

  useEffect(() => {
    (async () => {
      try {
        const s = await invokeSafe<TgSettingsView>("settings_get_tg");
        if (s.api_id) setApiId(String(s.api_id));
        if (s.api_hash) setApiHash(s.api_hash);
        if (s.tdlib_path) setTdlibPath(s.tdlib_path);
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
          Укажи значения API_ID и API_HASH, полученные в Telegram. Если путь к TDLib пустой,
          приложение попробует скачать и собрать библиотеку автоматически.
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

      <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
        <button
          onClick={async () => {
            try {
              setSaving(true);
              setStatus("Сохраняю...");
              const id = parseInt(apiId.trim(), 10);
              if (!Number.isFinite(id) || id <= 0) {
                setStatus("API_ID должен быть положительным числом");
                setSaving(false);
                return;
              }
              if (!apiHash.trim()) {
                setStatus("API_HASH не может быть пустым");
                setSaving(false);
                return;
              }
              await invokeSafe("settings_set_tg", {
                apiId: id,
                apiHash: apiHash.trim(),
                tdlibPath: tdlibPath.trim() ? tdlibPath.trim() : null
              });
              await refreshSettings();
              await refreshAuth();
              setStatus("Сохранено. Можно продолжить авторизацию.");
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
              Сборка идет. Прогресс отображается в главном окне.
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
