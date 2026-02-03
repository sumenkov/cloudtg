import React, { useEffect, useMemo, useState } from "react";
import { useAppStore, DirNode, ChatItem, FileItem } from "../store/app";
import { listenSafe } from "../tauri";
import { getCurrentWindow } from "@tauri-apps/api/window";

function containsNode(root: DirNode, id: string): boolean {
  if (root.id === id) return true;
  for (const c of root.children) {
    if (containsNode(c, id)) return true;
  }
  return false;
}

function findNode(root: DirNode, id: string): DirNode | null {
  if (root.id === id) return root;
  for (const c of root.children) {
    const found = findNode(c, id);
    if (found) return found;
  }
  return null;
}

function collectIds(node: DirNode, out: Set<string>) {
  out.add(node.id);
  for (const c of node.children) collectIds(c, out);
}

const REPAIR_NEED_FILE = "REPAIR_NEED_FILE";

type FlatNode = { id: string; label: string };

function flattenTree(node: DirNode, depth: number, out: FlatNode[], exclude: Set<string>) {
  if (!exclude.has(node.id)) {
    const prefix = depth > 0 ? "—".repeat(depth) + " " : "";
    const label = node.id === "ROOT" ? "Корень" : `${prefix}${node.name}`;
    out.push({ id: node.id, label });
  }
  for (const c of node.children) {
    flattenTree(c, depth + 1, out, exclude);
  }
}

type TreeRowProps = {
  node: DirNode;
  depth: number;
  selectedId: string | null;
  collapsed: Set<string>;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
};

function TreeRow({ node, depth, selectedId, collapsed, onSelect, onToggle }: TreeRowProps) {
  const hasChildren = node.children.length > 0;
  const isCollapsed = collapsed.has(node.id);
  const isRoot = node.id === "ROOT";
  const label = isRoot ? "Корень" : node.name;
  const marker = hasChildren ? (isCollapsed ? "▶" : "▼") : "";

  return (
    <div>
      <div
        role="treeitem"
        aria-expanded={hasChildren ? !isCollapsed : undefined}
        tabIndex={0}
        onClick={() => onSelect(node.id)}
        onFocus={() => onSelect(node.id)}
        onDoubleClick={() => hasChildren && onToggle(node.id)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && hasChildren) {
            e.preventDefault();
            onToggle(node.id);
          }
        }}
        style={{
          padding: "6px 8px",
          borderRadius: 10,
          cursor: "pointer",
          background: selectedId === node.id ? "#f3f3f3" : "transparent",
          marginLeft: depth * 16,
          display: "flex",
          alignItems: "center",
          gap: 6,
          userSelect: "none"
        }}
      >
        <button
          type="button"
          aria-label={hasChildren ? "Свернуть или развернуть папку" : "Нет подпапок"}
          disabled={!hasChildren}
          onClick={(e) => {
            e.stopPropagation();
            if (hasChildren) onToggle(node.id);
          }}
          style={{
            width: 20,
            height: 20,
            borderRadius: 6,
            border: "1px solid #ccc",
            background: "#fff",
            color: "#333",
            fontSize: 12,
            lineHeight: "16px",
            cursor: hasChildren ? "pointer" : "default",
            opacity: hasChildren ? 1 : 0.3
          }}
        >
          {marker || "•"}
        </button>
        <div style={{ display: "flex", alignItems: "center", gap: 6, flex: 1 }}>
          <span style={{ fontWeight: isRoot ? 700 : 500 }}>{label}</span>
          {!isRoot && node.is_broken ? (
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
              битая
            </span>
          ) : null}
        </div>
        {!isRoot && <span style={{ opacity: 0.5, fontSize: 12 }}>{node.id.slice(0, 6)}</span>}
      </div>
      {hasChildren && !isCollapsed && (
        <div role="group">
          {node.children.map((child) => (
            <TreeRow
              key={child.id}
              node={child}
              depth={depth + 1}
              selectedId={selectedId}
              collapsed={collapsed}
              onSelect={onSelect}
              onToggle={onToggle}
            />
          ))}
        </div>
      )}
    </div>
  );
}

export function FileManager({ tree }: { tree: DirNode | null }) {
  const {
    createDir,
    renameDir,
    moveDir,
    deleteDir,
    repairDir,
    files,
    refreshFiles,
    searchFiles,
    pickFiles,
    uploadFile,
    moveFiles,
    deleteFiles,
    repairFile,
    downloadFile,
    openFile,
    openFileFolder,
    searchChats,
    shareFileToChat,
    getRecentChats,
    setError
  } = useAppStore();
  const [parentId, setParentId] = useState<string | null>(tree?.id ?? "ROOT");
  const [name, setName] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [renameValue, setRenameValue] = useState("");
  const [moveParentId, setMoveParentId] = useState<string>("ROOT");
  const [selectedFiles, setSelectedFiles] = useState<Set<string>>(() => new Set());
  const [fileMoveTarget, setFileMoveTarget] = useState<string>("");
  const [dropActive, setDropActive] = useState(false);
  const [shareFile, setShareFile] = useState<FileItem | null>(null);
  const [shareQuery, setShareQuery] = useState("");
  const [shareResults, setShareResults] = useState<ChatItem[]>([]);
  const [shareBusy, setShareBusy] = useState(false);
  const [shareStatus, setShareStatus] = useState<string | null>(null);
  const [searchName, setSearchName] = useState("");
  const [searchType, setSearchType] = useState("");
  const [searchAll, setSearchAll] = useState(false);
  const [searchActive, setSearchActive] = useState(false);
  const [searchBusy, setSearchBusy] = useState(false);

  useEffect(() => {
    if (!tree) return;
    setParentId((current) => {
      if (!current) return tree.id;
      return containsNode(tree, current) ? current : tree.id;
    });
    setCollapsed((prev) => {
      const next = new Set<string>();
      for (const id of prev) {
        if (containsNode(tree, id)) next.add(id);
      }
      return next;
    });
  }, [tree]);

  const selectedNode = useMemo(() => {
    if (!tree || !parentId) return null;
    return findNode(tree, parentId);
  }, [tree, parentId]);

  const runSearch = async () => {
    if (!selectedNode) return;
    const name = searchName.trim();
    const fileType = searchType.trim();
    if (!name && !fileType) {
      setSearchActive(false);
      await refreshFiles(selectedNode.id);
      setSelectedFiles(new Set());
      return;
    }
    try {
      setSearchBusy(true);
      await searchFiles({
        dirId: searchAll ? null : selectedNode.id,
        name: name || undefined,
        fileType: fileType || undefined
      });
      setSearchActive(true);
      setSelectedFiles(new Set());
    } catch (e: any) {
      setError(String(e));
    } finally {
      setSearchBusy(false);
    }
  };

  const reloadFiles = async () => {
    if (!selectedNode) return;
    if (searchActive) {
      await runSearch();
    } else {
      await refreshFiles(selectedNode.id);
    }
  };

  useEffect(() => {
    if (!selectedNode) {
      setRenameValue("");
      setMoveParentId("ROOT");
      setSelectedFiles(new Set());
      setShareFile(null);
      setShareResults([]);
      setShareQuery("");
      setShareStatus(null);
      setSearchName("");
      setSearchType("");
      setSearchAll(false);
      setSearchActive(false);
      return;
    }
    setRenameValue(selectedNode.name);
    setMoveParentId(selectedNode.parent_id ?? "ROOT");
    setSelectedFiles(new Set());
    setShareFile(null);
    setShareResults([]);
    setShareQuery("");
    setShareStatus(null);
    setSearchName("");
    setSearchType("");
    setSearchAll(false);
    setSearchActive(false);
  }, [selectedNode]);

  useEffect(() => {
    if (!shareFile) return;
    setShareStatus(null);
    (async () => {
      try {
        setShareBusy(true);
        const recent = await getRecentChats();
        setShareResults(recent);
      } catch (e: any) {
        setError(String(e));
      } finally {
        setShareBusy(false);
      }
    })();
  }, [shareFile, getRecentChats, setError]);

  const moveOptions = useMemo(() => {
    if (!tree) return [];
    const exclude = new Set<string>();
    if (selectedNode) {
      collectIds(selectedNode, exclude);
    }
    const out: FlatNode[] = [];
    flattenTree(tree, 0, out, exclude);
    return out;
  }, [tree, selectedNode]);

  const isRootSelected = selectedNode?.id === "ROOT";
  const canUseFiles = Boolean(selectedNode && !isRootSelected);

  useEffect(() => {
    if (!selectedNode) return;
    refreshFiles(selectedNode.id).catch((e) => setError(String(e)));
  }, [selectedNode, refreshFiles, setError]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    (async () => {
      try {
        unlisten = await listenSafe("tree_updated", async () => {
          if (!selectedNode || isRootSelected) return;
          await reloadFiles();
        });
      } catch (e: any) {
        setError(String(e));
      }
    })();
    return () => {
      if (unlisten) unlisten();
    };
  }, [
    selectedNode,
    isRootSelected,
    searchActive,
    searchName,
    searchType,
    searchAll,
    reloadFiles,
    setError
  ]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    const win = getCurrentWindow();
    win
      .onDragDropEvent((event) => {
        const payload: any = event.payload;
        if (payload.type === "over") {
          setDropActive(true);
        } else if (payload.type === "leave") {
          setDropActive(false);
        } else if (payload.type === "drop") {
          setDropActive(false);
          const paths = payload.paths as string[] | undefined;
          if (!paths || paths.length === 0) return;
          if (!selectedNode || isRootSelected) {
            setError("Выбери папку, чтобы загрузить файлы.");
            return;
          }
          (async () => {
            try {
              for (const path of paths) {
                await uploadFile(selectedNode.id, path);
              }
              await reloadFiles();
            } catch (e: any) {
              setError(String(e));
            }
          })();
        }
      })
      .then((u) => {
        unlisten = u;
      })
      .catch(() => {
        // В браузере событие может быть недоступно.
      });
    return () => {
      if (unlisten) unlisten();
    };
  }, [selectedNode, isRootSelected, uploadFile, refreshFiles, setError]);

  const fileMoveOptions = useMemo(() => {
    if (!tree) return [];
    const exclude = new Set<string>();
    const out: FlatNode[] = [];
    for (const child of tree.children) {
      flattenTree(child, 0, out, exclude);
    }
    return out;
  }, [tree]);

  useEffect(() => {
    if (!canUseFiles || fileMoveOptions.length === 0) {
      setFileMoveTarget("");
      return;
    }
    if (!fileMoveTarget) {
      setFileMoveTarget(fileMoveOptions[0].id);
    }
  }, [canUseFiles, fileMoveOptions, fileMoveTarget]);

  const toggleCollapse = useMemo(
    () => (id: string) =>
      setCollapsed((prev) => {
        const next = new Set(prev);
        if (next.has(id)) {
          next.delete(id);
        } else {
          next.add(id);
        }
        return next;
      }),
    []
  );

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 2fr", gap: 16 }}>
      <div style={{ border: "1px solid #ddd", borderRadius: 12, padding: 12 }}>
        <b>Дерево директорий</b>
        <div style={{ marginTop: 6, fontSize: 12, opacity: 0.6 }}>
          Двойной клик или Enter — свернуть/развернуть.
        </div>
        <div style={{ marginTop: 10, maxHeight: 500, overflow: "auto" }}>
          {tree ? (
            <TreeRow
              node={tree}
              depth={0}
              selectedId={parentId}
              collapsed={collapsed}
              onSelect={setParentId}
              onToggle={toggleCollapse}
            />
          ) : (
            <div style={{ opacity: 0.6 }}>Дерево пока не загружено.</div>
          )}
        </div>
      </div>

      <div style={{ border: "1px solid #ddd", borderRadius: 12, padding: 12 }}>
        <b>Операции</b>
        <div style={{ marginTop: 10, display: "grid", gridTemplateColumns: "1fr 180px", gap: 10 }}>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Новая папка..."
            style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
          />
          <button
            onClick={async () => {
              if (!name.trim()) return;
              try {
                await createDir(parentId === "ROOT" ? null : parentId, name.trim());
                setName("");
              } catch (e: any) {
                setError(String(e));
              }
            }}
            style={{ padding: 10, borderRadius: 10 }}
          >
            Создать папку
          </button>
        </div>

        <div style={{ marginTop: 16, borderTop: "1px solid #eee", paddingTop: 12 }}>
          <b>Переименование</b>
          <div style={{ marginTop: 8, display: "grid", gridTemplateColumns: "1fr 180px", gap: 10 }}>
            <input
              value={renameValue}
              onChange={(e) => setRenameValue(e.target.value)}
              placeholder="Новое имя..."
              disabled={!selectedNode || isRootSelected}
              style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
            />
            <button
              onClick={async () => {
                if (!selectedNode || isRootSelected) return;
                if (!renameValue.trim()) return;
                try {
                  await renameDir(selectedNode.id, renameValue.trim());
                } catch (e: any) {
                  setError(String(e));
                }
              }}
              disabled={!selectedNode || isRootSelected}
              style={{ padding: 10, borderRadius: 10 }}
            >
              Переименовать
            </button>
          </div>
        </div>

        <div style={{ marginTop: 16, borderTop: "1px solid #eee", paddingTop: 12 }}>
          <b>Перемещение директории</b>
          <div style={{ marginTop: 8, display: "grid", gridTemplateColumns: "1fr 180px", gap: 10 }}>
            <select
              value={moveParentId}
              onChange={(e) => setMoveParentId(e.target.value)}
              disabled={!selectedNode || isRootSelected || moveOptions.length === 0}
              style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc", background: "#fff" }}
            >
              {moveOptions.map((opt) => (
                <option key={opt.id} value={opt.id}>
                  {opt.label}
                </option>
              ))}
            </select>
            <button
              onClick={async () => {
                if (!selectedNode || isRootSelected) return;
                const target = moveParentId === "ROOT" ? null : moveParentId;
                try {
                  await moveDir(selectedNode.id, target);
                } catch (e: any) {
                  setError(String(e));
                }
              }}
              disabled={!selectedNode || isRootSelected || moveOptions.length === 0}
              style={{ padding: 10, borderRadius: 10 }}
            >
              Переместить
            </button>
          </div>
        </div>

        <div style={{ marginTop: 16, borderTop: "1px solid #eee", paddingTop: 12 }}>
          <b>Удаление</b>
          <div style={{ marginTop: 8 }}>
            <button
              onClick={async () => {
                if (!selectedNode || isRootSelected) return;
                const ok = window.confirm("Удалить папку? Действие нельзя отменить.");
                if (!ok) return;
                try {
                  await deleteDir(selectedNode.id);
                  setParentId("ROOT");
                } catch (e: any) {
                  setError(String(e));
                }
              }}
              disabled={!selectedNode || isRootSelected}
              style={{ padding: 10, borderRadius: 10, background: "#fee", border: "1px solid #f99" }}
            >
              Удалить папку
            </button>
            <div style={{ marginTop: 6, fontSize: 12, opacity: 0.6 }}>
              Удалять можно только пустые папки.
            </div>
          </div>
        </div>

        {selectedNode && !isRootSelected && selectedNode.is_broken ? (
          <div style={{ marginTop: 16, borderTop: "1px solid #eee", paddingTop: 12 }}>
            <b>Восстановление</b>
            <div style={{ marginTop: 8 }}>
              <button
                onClick={async () => {
                  try {
                    const res = await repairDir(selectedNode.id);
                    if (!res.ok) {
                      setError(res.message);
                    }
                  } catch (e: any) {
                    setError(String(e));
                  }
                }}
                style={{ padding: 10, borderRadius: 10 }}
              >
                Восстановить папку
              </button>
              <div style={{ marginTop: 6, fontSize: 12, opacity: 0.6 }}>
                Пересоздает/обновляет сообщение папки в канале хранения.
              </div>
            </div>
          </div>
        ) : null}

        <div style={{ marginTop: 16, borderTop: "1px solid #eee", paddingTop: 12 }}>
          <b>Файлы</b>
          {!canUseFiles ? (
            <div style={{ marginTop: 8, fontSize: 12, opacity: 0.6 }}>
              Выбери папку в дереве, чтобы управлять файлами.
            </div>
          ) : (
            <>
              <div style={{ marginTop: 8, display: "flex", gap: 10, alignItems: "center" }}>
                <div
                  style={{
                    flex: 1,
                    border: "1px dashed #bbb",
                    borderRadius: 12,
                    padding: "10px 12px",
                    background: dropActive ? "#f0f7ff" : "#fafafa",
                    color: "#333"
                  }}
                >
                  {dropActive
                    ? "Отпускай файлы, чтобы загрузить"
                    : "Перетащи файлы сюда для загрузки"}
                </div>
                <button
                  onClick={async () => {
                    if (!selectedNode || isRootSelected) return;
                    try {
                      const paths = await pickFiles();
                      if (!paths || paths.length === 0) return;
                      for (const path of paths) {
                        await uploadFile(selectedNode.id, path);
                      }
                      await reloadFiles();
                    } catch (e: any) {
                      setError(String(e));
                    }
                  }}
                  style={{ padding: 10, borderRadius: 10 }}
                >
                  Выбрать и загрузить
                </button>
                <div style={{ fontSize: 12, opacity: 0.6 }}>
                  Всего: {files.length}
                </div>
              </div>

                <div style={{ marginTop: 10, padding: 12, border: "1px solid #eee", borderRadius: 12, background: "#fafafa" }}>
                  <b>Поиск</b>
                <div style={{ marginTop: 8, display: "grid", gridTemplateColumns: "1fr 1fr 160px", gap: 8 }}>
                  <input
                    value={searchName}
                    onChange={(e) => setSearchName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        runSearch();
                      }
                    }}
                    placeholder="Имя"
                    style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
                  />
                  <input
                    value={searchType}
                    onChange={(e) => setSearchType(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        runSearch();
                      }
                    }}
                    placeholder="Тип (например pdf)"
                    style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
                  />
                  <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, opacity: 0.8 }}>
                    <input
                      type="checkbox"
                      checked={searchAll}
                      onChange={(e) => setSearchAll(e.target.checked)}
                    />
                    Во всех папках
                  </label>
                </div>
                <div style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center" }}>
                  <button
                    onClick={runSearch}
                    disabled={searchBusy}
                    style={{ padding: "6px 10px", borderRadius: 8 }}
                  >
                    Найти
                  </button>
                  <button
                    onClick={async () => {
                      if (!selectedNode) return;
                      setSearchName("");
                      setSearchType("");
                      setSearchAll(false);
                      setSearchActive(false);
                      setSelectedFiles(new Set());
                      try {
                        await refreshFiles(selectedNode.id);
                      } catch (e: any) {
                        setError(String(e));
                      }
                    }}
                    disabled={searchBusy}
                    style={{ padding: "6px 10px", borderRadius: 8, opacity: searchActive ? 1 : 0.7 }}
                  >
                    Сброс
                  </button>
                  {searchActive ? (
                    <div style={{ fontSize: 12, opacity: 0.6 }}>
                      Найдено: {files.length}
                    </div>
                  ) : null}
                </div>
              </div>


              <div style={{ marginTop: 10, display: "grid", gridTemplateColumns: "1fr 180px", gap: 10 }}>
                <select
                  value={fileMoveTarget}
                  onChange={(e) => setFileMoveTarget(e.target.value)}
                  disabled={fileMoveOptions.length === 0}
                  style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc", background: "#fff" }}
                >
                  <option value="" disabled>
                    Куда переместить…
                  </option>
                  {fileMoveOptions.map((opt) => (
                    <option key={opt.id} value={opt.id}>
                      {opt.label}
                    </option>
                  ))}
                </select>
                <button
                  onClick={async () => {
                    if (!selectedNode) return;
                    const ids = Array.from(selectedFiles);
                    if (ids.length === 0 || !fileMoveTarget) return;
                    try {
                      await moveFiles(ids, fileMoveTarget);
                      await reloadFiles();
                      setSelectedFiles(new Set());
                    } catch (e: any) {
                      setError(String(e));
                    }
                  }}
                  disabled={!fileMoveTarget || selectedFiles.size === 0}
                  style={{ padding: 10, borderRadius: 10 }}
                >
                  Переместить выбранные
                </button>
              </div>

              <div style={{ marginTop: 10, display: "flex", gap: 10, alignItems: "center" }}>
                <button
                  onClick={async () => {
                    if (!selectedNode) return;
                    const ids = Array.from(selectedFiles);
                    if (ids.length === 0) return;
                    const ok = window.confirm(`Удалить файлов: ${ids.length}?`);
                    if (!ok) return;
                    try {
                      await deleteFiles(ids);
                      await reloadFiles();
                      setSelectedFiles(new Set());
                    } catch (e: any) {
                      setError(String(e));
                    }
                  }}
                  disabled={selectedFiles.size === 0}
                  style={{ padding: 10, borderRadius: 10, background: "#fee", border: "1px solid #f99" }}
                >
                  Удалить выбранные
                </button>
                <div style={{ fontSize: 12, opacity: 0.6 }}>
                  {selectedFiles.size > 0 ? `Выбрано: ${selectedFiles.size}` : "Выбери файлы для действий"}
                </div>
              </div>

              {shareStatus ? (
                <div style={{ marginTop: 10, padding: 10, borderRadius: 8, background: "#f6f6f6" }}>
                  {shareStatus}
                </div>
              ) : null}

              {shareFile ? (
                <div style={{ marginTop: 12, padding: 12, border: "1px solid #ddd", borderRadius: 10 }}>
                  <b>Поделиться файлом</b>
                  <div style={{ marginTop: 6, fontSize: 12, opacity: 0.7 }}>
                    Файл: {shareFile.name}
                  </div>
                  <div style={{ marginTop: 8, display: "grid", gridTemplateColumns: "1fr 160px", gap: 8 }}>
                    <input
                      value={shareQuery}
                      onChange={(e) => setShareQuery(e.target.value)}
                      placeholder="Название чата или @username"
                      style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
                    />
                    <button
                      onClick={async () => {
                        const q = shareQuery.trim();
                        if (!q) return;
                        try {
                          setShareBusy(true);
                          const res = await searchChats(q);
                          setShareResults(res);
                        } catch (e: any) {
                          setError(String(e));
                        } finally {
                          setShareBusy(false);
                        }
                      }}
                      disabled={shareBusy || !shareQuery.trim()}
                      style={{ padding: 10, borderRadius: 10 }}
                    >
                      Найти
                    </button>
                  </div>
                  <div style={{ marginTop: 8, display: "flex", gap: 8, flexWrap: "wrap" }}>
                    <button
                      onClick={() => {
                        setShareFile(null);
                        setShareQuery("");
                        setShareResults([]);
                      }}
                      style={{ padding: "6px 10px", borderRadius: 8, opacity: 0.8 }}
                    >
                      Закрыть
                    </button>
                    <button
                      onClick={async () => {
                        try {
                          setShareBusy(true);
                          const recent = await getRecentChats();
                          setShareResults(recent);
                        } catch (e: any) {
                          setError(String(e));
                        } finally {
                          setShareBusy(false);
                        }
                      }}
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
                              onClick={async () => {
                                if (!shareFile) return;
                                try {
                                  setShareBusy(true);
                                  const msg = await shareFileToChat(shareFile.id, chat.id);
                                  setShareStatus(msg);
                                  setShareFile(null);
                                  setShareResults([]);
                                  setShareQuery("");
                                } catch (e: any) {
                                  setError(String(e));
                                } finally {
                                  setShareBusy(false);
                                }
                              }}
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
              ) : null}

              <div style={{ marginTop: 12, border: "1px solid #eee", borderRadius: 10, overflow: "hidden" }}>
                {files.length === 0 ? (
                  <div style={{ padding: 12, fontSize: 12, opacity: 0.6 }}>Файлов пока нет.</div>
                ) : (
                  <div>
                    {files.map((f) => {
                      const checked = selectedFiles.has(f.id);
                      return (
                        <div
                          key={f.id}
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
                            onChange={() => {
                              setSelectedFiles((prev) => {
                                const next = new Set(prev);
                                if (next.has(f.id)) next.delete(f.id);
                                else next.add(f.id);
                                return next;
                              });
                            }}
                          />
                          <div style={{ display: "flex", flexDirection: "column" }}>
                            <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                              <span style={{ fontWeight: 500 }}>{f.name}</span>
                              {f.is_broken ? (
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
                            <span style={{ fontSize: 11, opacity: 0.6 }}>#{f.hash}</span>
                          </div>
                          <div style={{ fontSize: 12, opacity: 0.7 }}>{formatBytes(f.size)}</div>
                          <div style={{ fontSize: 12, opacity: 0.5 }}>{f.id.slice(0, 6)}</div>
                          <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, flexWrap: "wrap" }}>
                            <button
                              onClick={async () => {
                                try {
                                  await downloadFile(f.id);
                                } catch (e: any) {
                                  setError(String(e));
                                }
                              }}
                              style={{ padding: "6px 10px", borderRadius: 8 }}
                            >
                              Скачать
                            </button>
                            <button
                              onClick={async () => {
                                try {
                                  await openFile(f.id);
                                } catch (e: any) {
                                  setError(String(e));
                                }
                              }}
                              style={{ padding: "6px 10px", borderRadius: 8 }}
                            >
                              Открыть
                            </button>
                            <button
                              onClick={async () => {
                                try {
                                  await openFileFolder(f.id);
                                } catch (e: any) {
                                  setError(String(e));
                                }
                              }}
                              style={{ padding: "6px 10px", borderRadius: 8 }}
                            >
                              Открыть папку
                            </button>
                            <button
                              onClick={() => {
                                setShareFile(f);
                                setShareStatus(null);
                              }}
                              style={{ padding: "6px 10px", borderRadius: 8 }}
                            >
                              Поделиться
                            </button>
                            {f.is_broken ? (
                              <button
                                onClick={async () => {
                                  try {
                                    let res = await repairFile(f.id);
                                    if (!res.ok && res.code === REPAIR_NEED_FILE) {
                                      const paths = await pickFiles();
                                      if (!paths || paths.length === 0) return;
                                      res = await repairFile(f.id, paths[0]);
                                    }
                                    if (!res.ok) {
                                      setError(res.message);
                                      return;
                                    }
                                    await reloadFiles();
                                  } catch (e: any) {
                                    setError(String(e));
                                  }
                                }}
                                style={{ padding: "6px 10px", borderRadius: 8 }}
                              >
                                Восстановить
                              </button>
                            ) : null}
                            <button
                              onClick={async () => {
                                const ok = window.confirm("Удалить файл?");
                                if (!ok) return;
                                try {
                                  await deleteFiles([f.id]);
                                  if (selectedNode) await reloadFiles();
                                  setSelectedFiles((prev) => {
                                    const next = new Set(prev);
                                    next.delete(f.id);
                                    return next;
                                  });
                                } catch (e: any) {
                                  setError(String(e));
                                }
                              }}
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
            </>
          )}
        </div>
      </div>
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
