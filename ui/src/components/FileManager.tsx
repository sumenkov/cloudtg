import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useAppStore, DirNode, ChatItem, FileItem } from "../store/app";
import { listenSafe } from "../tauri";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { createDragDropHandler, createTreeUpdatedHandler, normalizeUploadPaths } from "./fileManagerListeners";
import { TreePanel } from "./file-manager/TreePanel";
import { SearchPanel } from "./file-manager/SearchPanel";
import { SharePanel } from "./file-manager/SharePanel";
import { FileList } from "./file-manager/FileList";
import { handleDownloadAction, handleOpenAction, handleOpenFolderAction } from "./file-manager/fileActions";

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
  const [uploadBusy, setUploadBusy] = useState(false);
  const selectedNodeRef = useRef<DirNode | null>(null);
  const isRootSelectedRef = useRef<boolean>(false);
  const reloadFilesRef = useRef<() => Promise<void>>(async () => {});
  const uploadInProgressRef = useRef<boolean>(false);

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

  const runSearch = useCallback(async () => {
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
  }, [selectedNode, searchName, searchType, refreshFiles, searchFiles, searchAll, setError]);

  const reloadFiles = useCallback(async () => {
    if (!selectedNode) return;
    if (searchActive) {
      await runSearch();
    } else {
      await refreshFiles(selectedNode.id);
    }
  }, [selectedNode, searchActive, runSearch, refreshFiles]);

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

  selectedNodeRef.current = selectedNode;
  isRootSelectedRef.current = isRootSelected;
  reloadFilesRef.current = reloadFiles;

  useEffect(() => {
    if (!selectedNode) return;
    refreshFiles(selectedNode.id).catch((e) => setError(String(e)));
  }, [selectedNode, refreshFiles, setError]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    const handleTreeUpdated = createTreeUpdatedHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef
    );
    (async () => {
      try {
        const cleanup = await listenSafe("tree_updated", handleTreeUpdated);
        if (disposed) {
          cleanup();
          return;
        }
        unlisten = cleanup;
      } catch (e: any) {
        setError(String(e));
      }
    })();
    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [setError]);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    const win = getCurrentWindow();
    const handleDragDropEvent = createDragDropHandler(
      selectedNodeRef,
      isRootSelectedRef,
      reloadFilesRef,
      uploadInProgressRef,
      uploadFile,
      setDropActive,
      setUploadBusy,
      (message) => setError(message)
    );
    win
      .onDragDropEvent(handleDragDropEvent)
      .then((u) => {
        if (disposed) {
          u();
          return;
        }
        unlisten = u;
      })
      .catch(() => {
        // В браузере событие может быть недоступно.
      });
    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [uploadFile, setError]);

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
    const stillExists = fileMoveOptions.some((option) => option.id === fileMoveTarget);
    if (!fileMoveTarget || !stillExists) {
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
      <TreePanel
        tree={tree}
        selectedId={parentId}
        collapsed={collapsed}
        onSelect={setParentId}
        onToggle={toggleCollapse}
      />

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
                    if (!selectedNode || isRootSelected || uploadInProgressRef.current) return;
                    uploadInProgressRef.current = true;
                    setUploadBusy(true);
                    try {
                      const paths = await pickFiles();
                      const uniquePaths = normalizeUploadPaths(paths);
                      if (uniquePaths.length === 0) return;
                      for (const path of uniquePaths) {
                        await uploadFile(selectedNode.id, path);
                      }
                      await reloadFiles();
                    } catch (e: any) {
                      setError(String(e));
                    } finally {
                      uploadInProgressRef.current = false;
                      setUploadBusy(false);
                    }
                  }}
                  disabled={uploadBusy}
                  style={{ padding: 10, borderRadius: 10 }}
                >
                  {uploadBusy ? "Загрузка..." : "Выбрать и загрузить"}
                </button>
                <div style={{ fontSize: 12, opacity: 0.6 }}>
                  Всего: {files.length}
                </div>
              </div>

              <SearchPanel
                searchName={searchName}
                searchType={searchType}
                searchAll={searchAll}
                searchBusy={searchBusy}
                searchActive={searchActive}
                foundCount={files.length}
                onSearchNameChange={setSearchName}
                onSearchTypeChange={setSearchType}
                onSearchAllChange={setSearchAll}
                onRunSearch={runSearch}
                onReset={async () => {
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
              />


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

              <SharePanel
                shareFile={shareFile}
                shareQuery={shareQuery}
                shareResults={shareResults}
                shareBusy={shareBusy}
                onShareQueryChange={setShareQuery}
                onClose={() => {
                  setShareFile(null);
                  setShareQuery("");
                  setShareResults([]);
                }}
                onSearch={async () => {
                  const query = shareQuery.trim();
                  if (!query) return;
                  try {
                    setShareBusy(true);
                    const res = await searchChats(query);
                    setShareResults(res);
                  } catch (e: any) {
                    setError(String(e));
                  } finally {
                    setShareBusy(false);
                  }
                }}
                onLoadRecent={async () => {
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
                onSend={async (chatId) => {
                  if (!shareFile) return;
                  try {
                    setShareBusy(true);
                    const msg = await shareFileToChat(shareFile.id, chatId);
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
              />

              <FileList
                files={files}
                selectedFiles={selectedFiles}
                onToggleSelect={(fileId) => {
                  setSelectedFiles((prev) => {
                    const next = new Set(prev);
                    if (next.has(fileId)) next.delete(fileId);
                    else next.add(fileId);
                    return next;
                  });
                }}
                onDownload={async (file) => {
                  try {
                    await handleDownloadAction({
                      file,
                      confirm: (message) => window.confirm(message),
                      downloadFile,
                      reloadFiles
                    });
                  } catch (e: any) {
                    setError(String(e));
                  }
                }}
                onOpen={async (file) => {
                  try {
                    await handleOpenAction({
                      file,
                      openFile,
                      reloadFiles
                    });
                  } catch (e: any) {
                    setError(String(e));
                  }
                }}
                onOpenFolder={async (file) => {
                  try {
                    await handleOpenFolderAction({
                      file,
                      openFileFolder
                    });
                  } catch (e: any) {
                    setError(String(e));
                  }
                }}
                onShare={(file) => {
                  setShareFile(file);
                  setShareStatus(null);
                }}
                onRepair={async (file) => {
                  try {
                    let res = await repairFile(file.id);
                    if (!res.ok && res.code === REPAIR_NEED_FILE) {
                      const paths = await pickFiles();
                      if (!paths || paths.length === 0) return;
                      res = await repairFile(file.id, paths[0]);
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
                onDelete={async (file) => {
                  const ok = window.confirm("Удалить файл?");
                  if (!ok) return;
                  try {
                    await deleteFiles([file.id]);
                    if (selectedNode) await reloadFiles();
                    setSelectedFiles((prev) => {
                      const next = new Set(prev);
                      next.delete(file.id);
                      return next;
                    });
                  } catch (e: any) {
                    setError(String(e));
                  }
                }}
              />
            </>
          )}
        </div>
      </div>
    </div>
  );
}
