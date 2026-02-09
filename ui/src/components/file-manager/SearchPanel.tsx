import React from "react";
import { Hint } from "../common/Hint";

type SearchPanelProps = {
  searchName: string;
  searchType: string;
  searchAll: boolean;
  searchBusy: boolean;
  searchActive: boolean;
  foundCount: number;
  onSearchNameChange: (value: string) => void;
  onSearchTypeChange: (value: string) => void;
  onSearchAllChange: (value: boolean) => void;
  onRunSearch: () => void | Promise<void>;
  onReset: () => void | Promise<void>;
};

export function SearchPanel({
  searchName,
  searchType,
  searchAll,
  searchBusy,
  searchActive,
  foundCount,
  onSearchNameChange,
  onSearchTypeChange,
  onSearchAllChange,
  onRunSearch,
  onReset
}: SearchPanelProps) {
  return (
    <div style={{ marginTop: 10, padding: 12, border: "1px solid #eee", borderRadius: 12, background: "#fafafa" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
        <b>Поиск</b>
        <Hint text="Можно искать по имени и/или расширению. Если отметить «Во всех папках», поиск не ограничивается текущей папкой." />
      </div>
      <div style={{ marginTop: 8, display: "grid", gridTemplateColumns: "1fr 1fr 160px", gap: 8 }}>
        <input
          value={searchName}
          onChange={(e) => onSearchNameChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void onRunSearch();
            }
          }}
          placeholder="Имя"
          style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
        />
        <input
          value={searchType}
          onChange={(e) => onSearchTypeChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void onRunSearch();
            }
          }}
          placeholder="Тип (например pdf)"
          style={{ padding: 10, borderRadius: 10, border: "1px solid #ccc" }}
        />
        <label style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 12, opacity: 0.8 }}>
          <input
            type="checkbox"
            checked={searchAll}
            onChange={(e) => onSearchAllChange(e.target.checked)}
          />
          Во всех папках
        </label>
      </div>
      <div style={{ marginTop: 8, display: "flex", gap: 8, alignItems: "center" }}>
        <button
          onClick={() => void onRunSearch()}
          disabled={searchBusy}
          style={{ padding: "6px 10px", borderRadius: 8 }}
        >
          Найти
        </button>
        <button
          onClick={() => void onReset()}
          disabled={searchBusy}
          style={{ padding: "6px 10px", borderRadius: 8, opacity: searchActive ? 1 : 0.7 }}
        >
          Сброс
        </button>
        {searchActive ? (
          <div style={{ fontSize: 12, opacity: 0.6 }}>
            Найдено: {foundCount}
          </div>
        ) : null}
      </div>
    </div>
  );
}
