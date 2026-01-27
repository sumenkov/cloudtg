import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";

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

  setAuth: (v: State["auth"] | string) => void;
  setError: (v: string | null) => void;

  refreshAuth: () => Promise<string>;
  refreshTree: () => Promise<void>;
  createDir: (parentId: string | null, name: string) => Promise<void>;
};

export const useAppStore = create<State>((set, get) => ({
  auth: "unknown",
  tree: null,
  error: null,

  setAuth: (v) => set({ auth: v as any }),
  setError: (v) => set({ error: v }),

  refreshAuth: async () => {
    const status = await invoke<{ state: string }>("auth_status");
    set({ auth: status.state as any });
    return status.state;
  },

  refreshTree: async () => {
    const t = await invoke<DirNode>("dir_list_tree");
    set({ tree: t });
  },

  createDir: async (parentId, name) => {
    await invoke("dir_create", { parentId, name });
    await get().refreshTree();
  }
}));
