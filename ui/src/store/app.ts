import { create } from "zustand";
import { invokeSafe } from "../tauri";

export type DirNode = {
  id: string;
  name: string;
  parent_id: string | null;
  children: DirNode[];
};

type State = {
  auth: "unknown" | "wait_config" | "wait_phone" | "wait_code" | "wait_password" | "ready" | "closed";
  tree: DirNode | null;
  error: string | null;
  tdlibBuild: {
    state: string | null;
    message: string | null;
    detail: string | null;
    progress: number | null;
  };
  tgSync: {
    state: string | null;
    message: string | null;
    processed: number;
    total: number | null;
  };
  tdlibLogs: Array<{ stream: "stdout" | "stderr"; line: string }>;
  tgSettings: {
    api_id: number | null;
    api_hash: string | null;
    tdlib_path: string | null;
  };

  setAuth: (v: State["auth"] | string) => void;
  setError: (v: string | null) => void;
  setTdlibBuild: (v: State["tdlibBuild"]) => void;
  setTgSync: (v: State["tgSync"]) => void;
  clearTdlibLogs: () => void;
  pushTdlibLog: (stream: "stdout" | "stderr", line: string) => void;
  touchTdlibBuildOnLog: () => void;

  refreshAuth: () => Promise<string>;
  refreshSettings: () => Promise<void>;
  refreshTree: () => Promise<void>;
  createDir: (parentId: string | null, name: string) => Promise<void>;
};

export const useAppStore = create<State>((set, get) => ({
  auth: "unknown",
  tree: null,
  error: null,
  tdlibBuild: { state: null, message: null, detail: null, progress: null },
  tgSync: { state: null, message: null, processed: 0, total: null },
  tdlibLogs: [],
  tgSettings: { api_id: null, api_hash: null, tdlib_path: null },

  setAuth: (v) => set({ auth: v as any }),
  setError: (v) => set({ error: v }),
  setTdlibBuild: (v) =>
    set((s) => {
      let progress = s.tdlibBuild.progress;
      if (v.state !== s.tdlibBuild.state) {
        progress = null;
      }
      if (v.state === "success") {
        progress = 100;
      }
      return { tdlibBuild: { ...v, progress } };
    }),
  setTgSync: (v) => set({ tgSync: v }),
  clearTdlibLogs: () => set({ tdlibLogs: [] }),
  touchTdlibBuildOnLog: () =>
    set((s) => {
      if (s.tdlibBuild.state) return s;
      return { tdlibBuild: { state: "build", message: "Идет сборка TDLib", detail: null, progress: null } };
    }),
  pushTdlibLog: (stream, line) =>
    set((s) => {
      let progress = s.tdlibBuild.progress;
      const parsed = extractPercent(line);
      if (parsed !== null) {
        if (progress === null || parsed >= progress) {
          progress = parsed;
        }
      }
      const next = [...s.tdlibLogs, { stream, line }];
      return { tdlibLogs: next.slice(-200), tdlibBuild: { ...s.tdlibBuild, progress } };
    }),

  refreshAuth: async () => {
    const status = await invokeSafe<{ state: string }>("auth_status");
    set({ auth: status.state as any });
    return status.state;
  },
  refreshSettings: async () => {
    const s = await invokeSafe<{ api_id: number | null; api_hash: string | null; tdlib_path: string | null }>(
      "settings_get_tg"
    );
    set({ tgSettings: s });
  },

  refreshTree: async () => {
    const t = await invokeSafe<DirNode>("dir_list_tree");
    set({ tree: t });
  },

  createDir: async (parentId, name) => {
    await invokeSafe("dir_create", { parentId, name });
    await get().refreshTree();
  }
}));

function extractPercent(line: string): number | null {
  const match = line.match(/(?:^|[^0-9])(\d{1,3})%/);
  if (!match) return null;
  const value = Number.parseInt(match[1], 10);
  if (!Number.isFinite(value) || value < 0 || value > 100) return null;
  return value;
}
