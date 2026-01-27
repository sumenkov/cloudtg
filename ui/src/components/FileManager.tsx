import React, { useMemo, useState } from "react";
import { useAppStore, DirNode } from "../store/app";

function flatten(node: DirNode | null): DirNode[] {
  if (!node) return [];
  const out: DirNode[] = [];
  const walk = (n: DirNode) => {
    out.push(n);
    n.children.forEach(walk);
  };
  walk(node);
  return out;
}

export function FileManager({ tree }: { tree: DirNode | null }) {
  const { createDir } = useAppStore();
  const nodes = useMemo(() => flatten(tree), [tree]);
  const [parentId, setParentId] = useState<string | null>(tree?.id ?? "ROOT");
  const [name, setName] = useState("");

  return (
    <div style={{ display: "grid", gridTemplateColumns: "1fr 2fr", gap: 16 }}>
      <div style={{ border: "1px solid #ddd", borderRadius: 12, padding: 12 }}>
        <b>Дерево директорий</b>
        <div style={{ marginTop: 10, maxHeight: 500, overflow: "auto" }}>
          {nodes.map((n) => (
            <div
              key={n.id}
              style={{
                padding: "6px 8px",
                borderRadius: 10,
                cursor: "pointer",
                background: parentId === n.id ? "#f3f3f3" : "transparent"
              }}
              onClick={() => setParentId(n.id)}
            >
              <span style={{ opacity: 0.7, marginRight: 8 }}>{n.parent_id ? "↳" : "◆"}</span>
              {n.name}
              <span style={{ float: "right", opacity: 0.5, fontSize: 12 }}>{n.id.slice(0, 6)}</span>
            </div>
          ))}
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
              await createDir(parentId === "ROOT" ? null : parentId, name.trim());
              setName("");
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
