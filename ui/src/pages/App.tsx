import React, { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invokeSafe, listenSafe, isTauri } from "../tauri";
import { useAppStore } from "../store/app";
import { FileManager } from "../components/FileManager";
import { Login } from "../components/Login";
import { Settings } from "../components/Settings";
import { Hint } from "../components/common/Hint";
import {
  createAuthStateChangedHandler,
  createListenerRegistrar,
  disposeListeners,
  runSyncOnce
} from "./appLifecycle";

type AppUpdateInfo = {
  current_version: string;
  latest_version: string | null;
  has_update: boolean;
  download_url: string | null;
  release_url: string | null;
};

type HelpBlock =
  | { type: "heading"; level: 1 | 2 | 3; text: string }
  | { type: "paragraph"; text: string }
  | { type: "list"; ordered: boolean; items: string[] }
  | { type: "code"; text: string };

function parseHelpMarkdown(text: string): HelpBlock[] {
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  const blocks: HelpBlock[] = [];

  let paragraphLines: string[] = [];
  let listType: null | "ul" | "ol" = null;
  let listItems: string[] = [];
  let inCode = false;
  let codeLines: string[] = [];

  const flushParagraph = () => {
    if (paragraphLines.length === 0) return;
    blocks.push({ type: "paragraph", text: paragraphLines.join(" ").trim() });
    paragraphLines = [];
  };

  const flushList = () => {
    if (!listType || listItems.length === 0) return;
    blocks.push({ type: "list", ordered: listType === "ol", items: [...listItems] });
    listType = null;
    listItems = [];
  };

  const flushCode = () => {
    if (codeLines.length === 0) return;
    blocks.push({ type: "code", text: codeLines.join("\n") });
    codeLines = [];
  };

  for (const line of lines) {
    const trimmed = line.trim();

    if (trimmed.startsWith("```")) {
      if (inCode) {
        flushCode();
        inCode = false;
      } else {
        flushParagraph();
        flushList();
        inCode = true;
      }
      continue;
    }

    if (inCode) {
      codeLines.push(line);
      continue;
    }

    if (!trimmed) {
      flushParagraph();
      flushList();
      continue;
    }

    const headingMatch = line.match(/^(#{1,3})\s+(.+)$/);
    if (headingMatch) {
      flushParagraph();
      flushList();
      const level = headingMatch[1].length as 1 | 2 | 3;
      blocks.push({ type: "heading", level, text: headingMatch[2].trim() });
      continue;
    }

    const ulMatch = line.match(/^\s*-\s+(.+)$/);
    if (ulMatch) {
      flushParagraph();
      if (listType !== "ul") {
        flushList();
        listType = "ul";
      }
      listItems.push(ulMatch[1].trim());
      continue;
    }

    const olMatch = line.match(/^\s*\d+\.\s+(.+)$/);
    if (olMatch) {
      flushParagraph();
      if (listType !== "ol") {
        flushList();
        listType = "ol";
      }
      listItems.push(olMatch[1].trim());
      continue;
    }

    flushList();
    paragraphLines.push(trimmed);
  }

  if (inCode) {
    flushCode();
  }
  flushParagraph();
  flushList();

  return blocks;
}

function renderInlineMarkdown(text: string, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const tokenRegex = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let index = 0;

  while ((match = tokenRegex.exec(text)) !== null) {
    const token = match[0];
    const start = match.index;
    if (start > last) {
      nodes.push(text.slice(last, start));
    }
    if (token.startsWith("`")) {
      nodes.push(
        <code key={`${keyPrefix}-code-${index}`} className="help-md-inline-code">
          {token.slice(1, -1)}
        </code>
      );
    } else if (token.startsWith("**")) {
      nodes.push(
        <strong key={`${keyPrefix}-strong-${index}`} className="help-md-strong">
          {token.slice(2, -2)}
        </strong>
      );
    } else {
      nodes.push(token);
    }
    last = start + token.length;
    index += 1;
  }

  if (last < text.length) {
    nodes.push(text.slice(last));
  }

  return nodes;
}

function renderHelpMarkdown(text: string): React.ReactNode {
  const blocks = parseHelpMarkdown(text);

  return (
    <div className="help-md-root">
      {blocks.map((block, index) => {
        const key = `help-${index}`;
        if (block.type === "heading") {
          if (block.level === 1) {
            return (
              <h1 key={key} className="help-md-h1">
                {renderInlineMarkdown(block.text, key)}
              </h1>
            );
          }
          if (block.level === 2) {
            return (
              <h2 key={key} className="help-md-h2">
                {renderInlineMarkdown(block.text, key)}
              </h2>
            );
          }
          return (
            <h3 key={key} className="help-md-h3">
              {renderInlineMarkdown(block.text, key)}
            </h3>
          );
        }

        if (block.type === "paragraph") {
          return (
            <p key={key} className="help-md-p">
              {renderInlineMarkdown(block.text, key)}
            </p>
          );
        }

        if (block.type === "code") {
          return (
            <pre key={key} className="help-md-pre">
              <code>{block.text}</code>
            </pre>
          );
        }

        const ListTag = block.ordered ? "ol" : "ul";
        return (
          <ListTag key={key} className={block.ordered ? "help-md-ol" : "help-md-ul"}>
            {block.items.map((item, itemIndex) => (
              <li key={`${key}-${itemIndex}`} className="help-md-li">
                {renderInlineMarkdown(item, `${key}-${itemIndex}`)}
              </li>
            ))}
          </ListTag>
        );
      })}
    </div>
  );
}

export default function App() {
  const {
    auth,
    setAuth,
    tree,
    refreshTree,
    error,
    setError,
    refreshAuth,
    refreshSettings,
    setTdlibBuild,
    clearTdlibLogs,
    pushTdlibLog,
    touchTdlibBuildOnLog,
    tdlibBuild,
    tgSync,
    setTgSync
  } = useAppStore();
  const [showSettings, setShowSettings] = useState(false);
  const [logoutBusy, setLogoutBusy] = useState(false);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppUpdateInfo | null>(null);
  const [showHelp, setShowHelp] = useState(false);
  const [helpBusy, setHelpBusy] = useState(false);
  const [helpText, setHelpText] = useState("");
  const syncStartedRef = useRef(false);
  const progressValue =
    tdlibBuild.progress === null ? null : Math.max(0, Math.min(100, tdlibBuild.progress));
  const syncProgressValue =
    tgSync.total && tgSync.total > 0 ? Math.max(0, Math.min(100, Math.floor((tgSync.processed / tgSync.total) * 100))) : null;
  const clearErrorOnUserActionStart = useCallback(() => {
    if (!error) return;
    setError(null);
  }, [error, setError]);
  const handleLogout = useCallback(async () => {
    if (!window.confirm("Выйти из Telegram в CloudTG?")) {
      return;
    }
    try {
      setLogoutBusy(true);
      setError(null);
      await invokeSafe("auth_logout");
      setShowSettings(false);
      setTgSync({ state: null, message: null, processed: 0, total: null });
      await refreshAuth();
      await refreshSettings();
    } catch (e: any) {
      setError(String(e));
    } finally {
      setLogoutBusy(false);
    }
  }, [refreshAuth, refreshSettings, setError, setTgSync]);
  const handleOpenHelp = useCallback(async () => {
    try {
      setHelpBusy(true);
      if (!helpText.trim()) {
        const text = await invokeSafe<string>("app_help_text");
        setHelpText(text);
      }
      setShowHelp(true);
    } catch (e: any) {
      setError(String(e));
    } finally {
      setHelpBusy(false);
    }
  }, [helpText, setError]);

  useEffect(() => {
    const disposedRef = { current: false };
    const unlisteners: Array<() => void> = [];
    const addListener = createListenerRegistrar(listenSafe, disposedRef, unlisteners);

    (async () => {
      try {
        const state = await refreshAuth();
        if (disposedRef.current) return;
        await refreshSettings();
        if (disposedRef.current) return;
        await refreshTree();
        if (disposedRef.current) return;

        if (state === "ready") {
          await runSyncOnce({
            syncStartedRef,
            invoke: invokeSafe,
            refreshTree,
            setError,
            disposedRef
          });
        }

        await addListener(
          "auth_state_changed",
          createAuthStateChangedHandler({
            disposedRef,
            syncStartedRef,
            setAuth,
            refreshTree,
            invoke: invokeSafe,
            setError
          })
        );

        await addListener<{ state: string; message: string; detail?: string | null }>("tdlib_build_status", async (event) => {
          if (disposedRef.current) return;
          if (event.payload.state === "start") {
            clearTdlibLogs();
          }
          setTdlibBuild({
            state: event.payload.state ?? null,
            message: event.payload.message ?? null,
            detail: event.payload.detail ?? null,
            progress: null
          });
        });

        await addListener<{ stream: "stdout" | "stderr"; line: string }>("tdlib_build_log", async (event) => {
          if (disposedRef.current) return;
          touchTdlibBuildOnLog();
          pushTdlibLog(event.payload.stream, event.payload.line);
        });

        await addListener<{ state: string; message: string; processed: number; total: number | null }>("tg_sync_status", async (event) => {
          if (disposedRef.current) return;
          setTgSync({
            state: event.payload.state ?? null,
            message: event.payload.message ?? null,
            processed: event.payload.processed ?? 0,
            total: event.payload.total ?? null
          });
        });

        await addListener<unknown>("tree_updated", async () => {
          if (disposedRef.current) return;
          await refreshTree();
        });
      } catch (e: any) {
        if (!disposedRef.current) {
          setError(String(e));
        }
      }
    })();

    return () => {
      disposeListeners(disposedRef, unlisteners);
    };
  }, [
    refreshAuth,
    refreshSettings,
    refreshTree,
    setAuth,
    setError,
    setTdlibBuild,
    clearTdlibLogs,
    pushTdlibLog,
    touchTdlibBuildOnLog,
    setTgSync
  ]);

  useEffect(() => {
    let active = true;
    (async () => {
      if (!isTauri()) return;
      try {
        const version = await getVersion();
        if (active) {
          setAppVersion(version);
        }
      } catch {
        // Ignore version read errors, UI can work without it.
      }
      try {
        const update = await invokeSafe<AppUpdateInfo>("app_check_update");
        if (active && update.has_update) {
          setAppUpdate(update);
        }
      } catch {
        // Ignore update check errors to avoid noisy UX when offline.
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  return (
    <div
      onPointerDownCapture={clearErrorOnUserActionStart}
      onKeyDownCapture={clearErrorOnUserActionStart}
      style={{ fontFamily: "system-ui, sans-serif", padding: 16, maxWidth: 1100, margin: "0 auto" }}
    >
      <style>
        {`
          @keyframes tgSyncMove {
            0% { transform: translateX(-60%); }
            50% { transform: translateX(60%); }
            100% { transform: translateX(120%); }
          }
          .help-md-root {
            color: #111827;
            font-size: 14px;
            line-height: 1.6;
          }
          .help-md-h1 {
            margin: 0 0 14px;
            font-size: 25px;
            line-height: 1.25;
          }
          .help-md-h2 {
            margin: 18px 0 10px;
            font-size: 19px;
            line-height: 1.3;
          }
          .help-md-h3 {
            margin: 14px 0 8px;
            font-size: 16px;
            line-height: 1.35;
          }
          .help-md-p {
            margin: 8px 0;
          }
          .help-md-ul, .help-md-ol {
            margin: 8px 0;
            padding-left: 22px;
          }
          .help-md-li {
            margin: 4px 0;
          }
          .help-md-inline-code {
            background: #f3f4f6;
            border: 1px solid #e5e7eb;
            border-radius: 6px;
            padding: 1px 6px;
            font-size: 12px;
            font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace;
          }
          .help-md-strong {
            font-weight: 650;
          }
          .help-md-pre {
            margin: 10px 0;
            border-radius: 10px;
            border: 1px solid #dbe3f0;
            background: #0f172a;
            color: #e5e7eb;
            padding: 12px;
            overflow: auto;
            font-size: 12px;
            line-height: 1.45;
          }
        `}
      </style>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
        <div>
          <h1 style={{ marginBottom: 4 }}>CloudTG</h1>
          {appVersion ? <div style={{ fontSize: 12, opacity: 0.65 }}>v{appVersion}</div> : null}
        </div>
        <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap", justifyContent: "flex-end" }}>
          <button
            onClick={() => {
              void handleOpenHelp();
            }}
            disabled={helpBusy}
            style={{ padding: "8px 12px", borderRadius: 10, opacity: helpBusy ? 0.7 : 1, cursor: helpBusy ? "wait" : "pointer" }}
          >
            {helpBusy ? "Загружаю..." : "Справка"}
          </button>
          {auth === "ready" ? (
            <button
              onClick={handleLogout}
              disabled={logoutBusy}
              style={{ padding: "8px 12px", borderRadius: 10, opacity: logoutBusy ? 0.7 : 1, cursor: logoutBusy ? "wait" : "pointer" }}
            >
              {logoutBusy ? "Выхожу..." : "Выйти"}
            </button>
          ) : null}
          <button
            onClick={async () => {
              if (showSettings) {
                try {
                  await refreshAuth();
                } catch (e: any) {
                  setError(String(e));
                }
                setShowSettings(false);
                return;
              }
              setShowSettings(true);
            }}
            disabled={logoutBusy}
            style={{ padding: "8px 12px", borderRadius: 10 }}
          >
            {showSettings ? "Закрыть" : "Настройки"}
          </button>
        </div>
      </div>
      <p style={{ marginTop: 0, opacity: 0.8 }}>
        Добро пожаловать в CloudTG. Здесь можно хранить, искать и открывать файлы из Telegram как в обычном файловом менеджере.
      </p>

      {appUpdate?.has_update ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: "1px solid #9fd39f",
            background: "#f4fff4"
          }}
        >
          <b>Доступна новая версия: {appUpdate.latest_version ?? "latest"}</b>
          <div style={{ marginTop: 4, fontSize: 12, opacity: 0.8 }}>
            Текущая версия: {appUpdate.current_version}
          </div>
          {appUpdate.download_url ? (
            <div style={{ marginTop: 8, display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
              <button
                onClick={async () => {
                  try {
                    await invokeSafe("app_open_url", { url: appUpdate.download_url });
                  } catch (e: any) {
                    setError(String(e));
                  }
                }}
                style={{ padding: "8px 12px", borderRadius: 10 }}
              >
                Скачать
              </button>
              <span style={{ fontSize: 12, opacity: 0.8 }}>{appUpdate.download_url}</span>
            </div>
          ) : null}
        </div>
      ) : null}

      {tdlibBuild.state && tdlibBuild.state !== "success" ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: tdlibBuild.state === "error" ? "1px solid #f99" : "1px solid #ddd",
            background: tdlibBuild.state === "error" ? "#fee" : "#fafafa"
          }}
        >
          <b>Подготовка TDLib</b>
          <div style={{ marginTop: 6 }}>{tdlibBuild.message ?? "Статус сборки"}</div>
          {["start", "clone", "configure", "build", "download"].includes(tdlibBuild.state ?? "") ? (
            <div style={{ marginTop: 8, height: 8, background: "#e5e5e5", borderRadius: 999 }}>
              <div
                style={{
                  width:
                    progressValue !== null
                      ? `${progressValue}%`
                      : tdlibBuild.state === "download"
                      ? "25%"
                      : tdlibBuild.state === "start"
                      ? "15%"
                      : tdlibBuild.state === "clone"
                      ? "35%"
                      : tdlibBuild.state === "configure"
                      ? "60%"
                      : "85%",
                  height: "100%",
                  background: "#4a90e2",
                  borderRadius: 999
                }}
              />
            </div>
          ) : null}
          {progressValue !== null ? (
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>Прогресс: {progressValue}%</div>
          ) : null}
          {["start", "clone", "configure", "build", "download"].includes(tdlibBuild.state ?? "") ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.8 }}>
              Пока подготовка не закончится, программа работать не будет.
            </div>
          ) : null}
          {tdlibBuild.state === "success" ? (
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>Сборка завершена успешно.</div>
          ) : null}
          {tdlibBuild.state === "error" && tdlibBuild.detail ? (
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>{tdlibBuild.detail}</div>
          ) : null}
        </div>
      ) : null}

      {tgSync.state && ["start", "progress"].includes(tgSync.state) ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: "1px solid #ddd",
            background: "#fafafa"
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <b>Синхронизация из Telegram</b>
            <Hint text="Синхронизация может идти в фоне. Можно продолжать работу с интерфейсом." />
          </div>
          <div style={{ marginTop: 6 }}>{tgSync.message ?? "Читаю сообщения канала"}</div>
          <div style={{ marginTop: 8, height: 8, background: "#e5e5e5", borderRadius: 999, overflow: "hidden" }}>
            <div
              style={{
                width: syncProgressValue !== null ? `${syncProgressValue}%` : "40%",
                height: "100%",
                background: "#4a90e2",
                borderRadius: 999,
                animation: syncProgressValue === null ? "tgSyncMove 1.4s ease-in-out infinite" : "none"
              }}
            />
          </div>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.8 }}>
            {syncProgressValue !== null ? `Прогресс: ${syncProgressValue}%` : `Обработано сообщений: ${tgSync.processed}`}
          </div>
          <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
            Если сообщений много, синхронизация может занять время.
          </div>
        </div>
      ) : null}

      {tgSync.state === "error" ? (
        <div style={{ background: "#fee", border: "1px solid #f99", padding: 12, borderRadius: 8, marginBottom: 12 }}>
          <b>Синхронизация не удалась.</b> Проверь логи и подключение к Telegram.
        </div>
      ) : null}

      {auth === "ready" && !showSettings ? (
        <div
          style={{
            marginBottom: 12,
            padding: 12,
            borderRadius: 10,
            border: "1px solid #ddd",
            background: "#f7f7f7",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            gap: 12,
            flexWrap: "wrap"
          }}
        >
          <div>
            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
              <b>Обновить из Telegram</b>
              <Hint text="Проверяем новые файлы и изменения в канале хранения." />
            </div>
            <div style={{ marginTop: 4, fontSize: 12, opacity: 0.7 }}>
              Считываем новые сообщения из канала и обновляем структуру файлов.
            </div>
          </div>
          <button
            onClick={async () => {
              try {
                await invokeSafe("tg_sync_storage");
                await refreshTree();
              } catch (e: any) {
                setError(String(e));
              }
            }}
            disabled={tgSync.state === "start" || tgSync.state === "progress"}
            style={{ padding: "10px 14px", borderRadius: 10 }}
          >
            Обновить сейчас
          </button>
        </div>
      ) : null}

      {auth === "ready" && !showSettings ? (
        <div style={{ marginBottom: 12, padding: 12, borderRadius: 10, border: "1px solid #eee", background: "#fcfcfc" }}>
          <b>Как начать работу</b>
          <ol style={{ marginTop: 8, marginBottom: 0, paddingLeft: 20, fontSize: 13, opacity: 0.8 }}>
            <li>Выбери папку в дереве слева.</li>
            <li>Во вкладке «Файлы» загрузи или открой нужный файл.</li>
            <li>Для поиска используй вкладку «Поиск».</li>
            <li>Чтобы отправить файл в чат, во вкладке «Файлы» открой меню «⋯ Действия» и нажми «Поделиться».</li>
          </ol>
        </div>
      ) : null}

      {error ? (
        <div style={{ background: "#fee", border: "1px solid #f99", padding: 12, borderRadius: 8 }}>
          <b>Ошибка:</b> {error}
        </div>
      ) : null}

      {showSettings ? (
        <Settings />
      ) : auth !== "ready" ? (
        <Login />
      ) : (
        <FileManager tree={tree} />
      )}

      {showHelp ? (
        <div
          onClick={() => setShowHelp(false)}
          style={{
            position: "fixed",
            inset: 0,
            background: "rgba(0, 0, 0, 0.35)",
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            padding: 16,
            zIndex: 1000
          }}
        >
          <div
            onClick={(event) => event.stopPropagation()}
            style={{
              width: "min(980px, 100%)",
              maxHeight: "min(85vh, 900px)",
              borderRadius: 12,
              border: "1px solid #d9d9d9",
              background: "#fff",
              boxShadow: "0 24px 80px rgba(0,0,0,0.24)",
              display: "grid",
              gridTemplateRows: "auto 1fr"
            }}
          >
            <div
              style={{
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
                gap: 12,
                padding: 12,
                borderBottom: "1px solid #eee"
              }}
            >
              <div>
                <b>Справка</b>
                <div style={{ marginTop: 2, fontSize: 12, opacity: 0.7 }}>
                  Функционал и навигация по CloudTG
                </div>
              </div>
              <button onClick={() => setShowHelp(false)} style={{ padding: "8px 12px", borderRadius: 10 }}>
                Закрыть
              </button>
            </div>
            <div style={{ padding: 14, overflow: "auto" }}>
              {helpText.trim() ? renderHelpMarkdown(helpText) : "Справка пока недоступна."}
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
