import React, { useEffect, useMemo, useState } from "react";
import { useAppStore, DirNode } from "../store/app";

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
        <span style={{ fontWeight: isRoot ? 700 : 500, flex: 1 }}>{label}</span>
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
  const { createDir, renameDir, moveDir, deleteDir, setError } = useAppStore();
  const [parentId, setParentId] = useState<string | null>(tree?.id ?? "ROOT");
  const [name, setName] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [renameValue, setRenameValue] = useState("");
  const [moveParentId, setMoveParentId] = useState<string>("ROOT");

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

  useEffect(() => {
    if (!selectedNode) {
      setRenameValue("");
      setMoveParentId("ROOT");
      return;
    }
    setRenameValue(selectedNode.name);
    setMoveParentId(selectedNode.parent_id ?? "ROOT");
  }, [selectedNode]);

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
          <b>Перемещение</b>
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
      </div>
    </div>
  );
}
