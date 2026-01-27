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
  clearTdlibLogs: () => void;
  pushTdlibLog: (stream: "stdout" | "stderr", line: string) => void;

  refreshAuth: () => Promise<string>;
  refreshSettings: () => Promise<void>;
  refreshTree: () => Promise<void>;
  createDir: (parentId: string | null, name: string) => Promise<void>;
};

export const useAppStore = create<State>((set, get) => ({
  auth: "unknown",
  tree: null,
  error: null,
  tdlibBuild: { state: null, message: null, detail: null },
  tdlibLogs: [],
  tgSettings: { api_id: null, api_hash: null, tdlib_path: null },

  setAuth: (v) => set({ auth: v as any }),
  setError: (v) => set({ error: v }),
  setTdlibBuild: (v) => set({ tdlibBuild: v }),
  clearTdlibLogs: () => set({ tdlibLogs: [] }),
  pushTdlibLog: (stream, line) =>
    set((s) => {
      const next = [...s.tdlibLogs, { stream, line }];
      return { tdlibLogs: next.slice(-200) };
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
