import React from "react";
import type { FileItem } from "../../store/app";

type FileListProps = {
  files: FileItem[];
  selectedFiles: Set<string>;
  onToggleSelect: (id: string) => void;
  onDownload: (file: FileItem) => void | Promise<void>;
  onOpen: (file: FileItem) => void | Promise<void>;
  onOpenFolder: (file: FileItem) => void | Promise<void>;
  onShare: (file: FileItem) => void;
  onRepair: (file: FileItem) => void | Promise<void>;
  onDelete: (file: FileItem) => void | Promise<void>;
};

export function FileList({
  files,
  selectedFiles,
  onToggleSelect,
  onDownload,
  onOpen,
  onOpenFolder,
  onShare,
  onRepair,
  onDelete
}: FileListProps) {
  return (
    <div style={{ marginTop: 12, border: "1px solid #eee", borderRadius: 10, overflow: "hidden" }}>
      {files.length === 0 ? (
        <div style={{ padding: 12, fontSize: 12, opacity: 0.6 }}>Файлов пока нет.</div>
      ) : (
        <div>
          {files.map((file) => {
            const checked = selectedFiles.has(file.id);
            const displaySize = file.is_downloaded ? (file.local_size ?? 0) : 0;
            return (
              <div
                key={file.id}
                style={{
                  display: "grid",
                  gridTemplateColumns: "24px 1fr 120px 120px 420px",
                  gap: 8,
                  alignItems: "center",
                  padding: "8px 10px",
                  borderTop: "1px solid #f0f0f0"
                }}
              >
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => onToggleSelect(file.id)}
                />
                <div style={{ display: "flex", flexDirection: "column" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                    <span style={{ fontWeight: 500 }}>{file.name}</span>
                    {file.is_broken ? (
                      <span
                        style={{
                          fontSize: 11,
                          color: "#b00020",
                          border: "1px solid #f2b8b8",
                          background: "#fff3f3",
                          borderRadius: 999,
                          padding: "1px 6px"
                        }}
                      >
                        битый
                      </span>
                    ) : null}
                  </div>
                  <span style={{ fontSize: 11, opacity: 0.6 }}>#{file.hash}</span>
                </div>
                <div style={{ fontSize: 12, opacity: 0.7 }}>{formatBytes(displaySize)}</div>
                <div style={{ fontSize: 12, opacity: 0.5 }}>{file.id.slice(0, 6)}</div>
                <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, flexWrap: "wrap" }}>
                  <button
                    onClick={() => void onDownload(file)}
                    style={{ padding: "6px 10px", borderRadius: 8 }}
                  >
                    Скачать
                  </button>
                  <button
                    onClick={() => void onOpen(file)}
                    style={{ padding: "6px 10px", borderRadius: 8 }}
                  >
                    Открыть
                  </button>
                  {file.is_downloaded ? (
                    <button
                      onClick={() => void onOpenFolder(file)}
                      style={{ padding: "6px 10px", borderRadius: 8 }}
                    >
                      Открыть папку
                    </button>
                  ) : null}
                  <button
                    onClick={() => onShare(file)}
                    style={{ padding: "6px 10px", borderRadius: 8 }}
                  >
                    Поделиться
                  </button>
                  {file.is_broken ? (
                    <button
                      onClick={() => void onRepair(file)}
                      style={{ padding: "6px 10px", borderRadius: 8 }}
                    >
                      Восстановить
                    </button>
                  ) : null}
                  <button
                    onClick={() => void onDelete(file)}
                    style={{ padding: "6px 10px", borderRadius: 8, background: "#fee", border: "1px solid #f99" }}
                  >
                    Удалить
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function formatBytes(size: number): string {
  if (!Number.isFinite(size)) return "0 Б";
  const units = ["Б", "КБ", "МБ", "ГБ"];
  let value = size;
  let idx = 0;
  while (value >= 1024 && idx < units.length - 1) {
    value /= 1024;
    idx += 1;
  }
  return `${value.toFixed(value < 10 && idx > 0 ? 1 : 0)} ${units[idx]}`;
}
