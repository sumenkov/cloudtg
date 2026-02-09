import React from "react";
import type { ChatItem, FileItem } from "../../store/app";
import { Hint } from "../common/Hint";

type SharePanelProps = {
  shareFile: FileItem | null;
  shareQuery: string;
  shareResults: ChatItem[];
  shareBusy: boolean;
  onShareQueryChange: (value: string) => void;
  onClose: () => void;
  onSearch: () => void | Promise<void>;
  onLoadRecent: () => void | Promise<void>;
  onSend: (chatId: number) => void | Promise<void>;
};

export function SharePanel({
  shareFile,
  shareQuery,
  shareResults,
  shareBusy,
  onShareQueryChange,
  onClose,
  onSearch,
  onLoadRecent,
  onSend
}: SharePanelProps) {
  if (!shareFile) {
    return null;
  }

  return (
    <div style={{ marginTop: 12, padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <b>Поделиться файлом</b>
        <Hint text="Файл отправляется как пересылка сообщения в выбранный чат. Локальная копия для этого не требуется." />
      </div>
      <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
        Файл: {shareFile.name}
      </div>
      <div style={{ marginTop: 8, display: "grid", gridTemplateColumns: "1fr 160px", gap: 8 }}>
        <input
          value={shareQuery}
          onChange={(e) => onShareQueryChange(e.target.value)}
          placeholder="Название чата или @username"
          style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
        />
        <button
          onClick={() => void onSearch()}
          disabled={shareBusy || !shareQuery.trim()}
          style={{ padding: 10, borderRadius: 10 }}
        >
          Найти
        </button>
      </div>
      <div style={{ marginTop: 8, display: "flex", gap: 8, flexWrap: "wrap" }}>
        <button
          onClick={onClose}
          style={{ padding: "6px 10px", borderRadius: 8, opacity: 0.8 }}
        >
          Закрыть
        </button>
        <button
          onClick={() => void onLoadRecent()}
          disabled={shareBusy}
          style={{ padding: "6px 10px", borderRadius: 8 }}
        >
          Недавние чаты
        </button>
      </div>
      <div style={{ marginTop: 8 }}>
        {shareResults.length === 0 ? (
          <div style={{ fontSize: 12, opacity: 0.6 }}>
            Введи запрос и нажми «Найти» или выбери из недавних.
          </div>
        ) : (
          <div style={{ display: "grid", gap: 6 }}>
            {shareResults.map((chat) => (
              <div
                key={chat.id}
                style={{
                  display: "grid",
                  gridTemplateColumns: "1fr 140px",
                  gap: 10,
                  alignItems: "center",
                  padding: "8px 10px",
                  border: "1px solid #eee",
                  borderRadius: 8
                }}
              >
                <div>
                  <div style={{ fontWeight: 500 }}>{chat.title}</div>
                  <div style={{ fontSize: 12, opacity: 0.6 }}>
                    {chat.kind}
                    {chat.username ? ` • @${chat.username}` : ""}
                  </div>
                </div>
                <button
                  onClick={() => void onSend(chat.id)}
                  disabled={shareBusy}
                  style={{ padding: "6px 10px", borderRadius: 8 }}
                >
                  Отправить
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
