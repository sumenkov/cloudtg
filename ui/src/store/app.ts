import { create } from "zustand";
import { invokeSafe } from "../tauri";

export type DirNode = {
  id: string;
  name: string;
  parent_id: string | null;
  children: DirNode[];
};

export type FileItem = {
  id: string;
  dir_id: string;
  name: string;
  size: number;
  hash: string;
  tg_chat_id: number;
  tg_msg_id: number;
  created_at: number;
};

export type ChatItem = {
  id: number;
  title: string;
  kind: string;
  username: string | null;
};

export type FileSearchFilters = {
  dirId?: string | null;
  name?: string;
  hash?: string;
  fileType?: string;
  limit?: number;
};

type State = {
  auth: "unknown" | "wait_config" | "wait_phone" | "wait_code" | "wait_password" | "ready" | "closed";
  tree: DirNode | null;
  files: FileItem[];
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
    tdlib_path: string | null;
    credentials: {
      available: boolean;
      source: string | null;
      keychain_available: boolean;
      encrypted_present: boolean;
      locked: boolean;
    };
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
  renameDir: (dirId: string, name: string) => Promise<void>;
  moveDir: (dirId: string, parentId: string | null) => Promise<void>;
  deleteDir: (dirId: string) => Promise<void>;
  refreshFiles: (dirId: string) => Promise<void>;
  searchFiles: (filters: FileSearchFilters) => Promise<void>;
  pickFiles: () => Promise<string[]>;
  uploadFile: (dirId: string, path: string) => Promise<void>;
  moveFiles: (fileIds: string[], dirId: string) => Promise<void>;
  deleteFiles: (fileIds: string[]) => Promise<void>;
  downloadFile: (fileId: string) => Promise<string>;
  openFile: (fileId: string) => Promise<void>;
  openFileFolder: (fileId: string) => Promise<void>;
  searchChats: (query: string) => Promise<ChatItem[]>;
  shareFileToChat: (fileId: string, chatId: number) => Promise<string>;
  getRecentChats: () => Promise<ChatItem[]>;
};

export const useAppStore = create<State>((set, get) => ({
  auth: "unknown",
  tree: null,
  files: [],
  error: null,
  tdlibBuild: { state: null, message: null, detail: null, progress: null },
  tgSync: { state: null, message: null, processed: 0, total: null },
  tdlibLogs: [],
  tgSettings: {
    tdlib_path: null,
    credentials: {
      available: false,
      source: null,
      keychain_available: true,
      encrypted_present: false,
      locked: false
    }
  },

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
    const s = await invokeSafe<{
      tdlib_path: string | null;
      credentials: {
        available: boolean;
        source: string | null;
        keychain_available: boolean;
        encrypted_present: boolean;
        locked: boolean;
      };
    }>("settings_get_tg");
    set({ tgSettings: s });
  },

  refreshTree: async () => {
    const t = await invokeSafe<DirNode>("dir_list_tree");
    set({ tree: t });
  },

  createDir: async (parentId, name) => {
    await invokeSafe("dir_create", { parentId, name });
    await get().refreshTree();
  },
  renameDir: async (dirId, name) => {
    await invokeSafe("dir_rename", { dirId, name });
    await get().refreshTree();
  },
  moveDir: async (dirId, parentId) => {
    await invokeSafe("dir_move", { dirId, parentId });
    await get().refreshTree();
  },
  deleteDir: async (dirId) => {
    await invokeSafe("dir_delete", { dirId });
    await get().refreshTree();
  },
  refreshFiles: async (dirId) => {
    const items = await invokeSafe<FileItem[]>("file_list", { dirId });
    set({ files: items });
  },
  searchFiles: async (filters) => {
    const items = await invokeSafe<FileItem[]>("file_search", filters);
    set({ files: items });
  },
  pickFiles: async () => {
    const files = await invokeSafe<string[]>("file_pick");
    return files;
  },
  uploadFile: async (dirId, path) => {
    await invokeSafe("file_upload", { dirId, path });
  },
  moveFiles: async (fileIds, dirId) => {
    for (const fileId of fileIds) {
      await invokeSafe("file_move", { fileId, dirId });
    }
  },
  deleteFiles: async (fileIds) => {
    if (fileIds.length === 0) return;
    if (fileIds.length === 1) {
      await invokeSafe("file_delete", { fileId: fileIds[0] });
    } else {
      await invokeSafe("file_delete_many", { fileIds });
    }
  },
  downloadFile: async (fileId) => {
    return invokeSafe<string>("file_download", { fileId });
  },
  openFile: async (fileId) => {
    await invokeSafe("file_open", { fileId });
  },
  openFileFolder: async (fileId) => {
    await invokeSafe("file_open_folder", { fileId });
  },
  searchChats: async (query) => {
    return invokeSafe<ChatItem[]>("tg_search_chats", { query });
  },
  shareFileToChat: async (fileId, chatId) => {
    const res = await invokeSafe<{ message: string }>("file_share_to_chat", { fileId, chatId });
    return res.message;
  },
  getRecentChats: async () => {
    return invokeSafe<ChatItem[]>("tg_recent_chats");
  }
}));

function extractPercent(line: string): number | null {
  const match = line.match(/(?:^|[^0-9])(\d{1,3})%/);
  if (!match) return null;
  const value = Number.parseInt(match[1], 10);
  if (!Number.isFinite(value) || value < 0 || value > 100) return null;
  return value;
}
