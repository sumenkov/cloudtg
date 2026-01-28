import React, { useEffect, useMemo, useState } from "react";
import { useAppStore, DirNode } from "../store/app";

function containsNode(root: DirNode, id: string): boolean {
  if (root.id === id) return true;
  for (const c of root.children) {
    if (containsNode(c, id)) return true;
  }
  return false;
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
  const { createDir, setError } = useAppStore();
  const [parentId, setParentId] = useState<string | null>(tree?.id ?? "ROOT");
  const [name, setName] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());

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

        <div style={{ marginTop: 14, opacity: 0.8 }}>
          Дальше: список файлов, upload/download, поиск, импорт.
        </div>
      </div>
    </div>
  );
}
