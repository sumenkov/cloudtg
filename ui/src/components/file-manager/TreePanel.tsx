import React from "react";
import type { DirNode } from "../../store/app";
import { Hint } from "../common/Hint";

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

type TreePanelProps = {
  tree: DirNode | null;
  selectedId: string | null;
  collapsed: Set<string>;
  onSelect: (id: string) => void;
  onToggle: (id: string) => void;
};

export function TreePanel({ tree, selectedId, collapsed, onSelect, onToggle }: TreePanelProps) {
  return (
    <div style={{ border: "1px solid #ddd", borderRadius: 12, padding: 12 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <b>Дерево папок</b>
        <Hint text="Выбери папку для работы. Двойной клик или Enter на строке сворачивает и разворачивает вложенные папки." />
      </div>
      <div style={{ marginTop: 6, fontSize: 12, opacity: 0.6 }}>
        Двойной клик или Enter — свернуть/развернуть.
      </div>
      <div style={{ marginTop: 10, maxHeight: 500, overflow: "auto" }}>
        {tree ? (
          <TreeRow
            node={tree}
            depth={0}
            selectedId={selectedId}
            collapsed={collapsed}
            onSelect={onSelect}
            onToggle={onToggle}
          />
        ) : (
          <div style={{ opacity: 0.6 }}>Дерево пока не загружено.</div>
        )}
      </div>
    </div>
  );
}
