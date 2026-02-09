import type { FileItem } from "../../store/app";

type DownloadActionArgs = {
  file: FileItem;
  confirm: (message: string) => boolean;
  downloadFile: (fileId: string, overwrite?: boolean) => Promise<string>;
  reloadFiles: () => Promise<void>;
};

type OpenActionArgs = {
  file: FileItem;
  openFile: (fileId: string) => Promise<void>;
  reloadFiles: () => Promise<void>;
};

type OpenFolderActionArgs = {
  file: FileItem;
  openFileFolder: (fileId: string) => Promise<void>;
};

export function displayFileSizeBytes(file: Pick<FileItem, "is_downloaded" | "local_size">): number {
  return file.is_downloaded ? (file.local_size ?? 0) : 0;
}

export function shouldShowOpenFolderButton(file: Pick<FileItem, "is_downloaded">): boolean {
  return file.is_downloaded;
}

export async function handleDownloadAction({
  file,
  confirm,
  downloadFile,
  reloadFiles
}: DownloadActionArgs): Promise<void> {
  let overwrite = false;
  if (file.is_downloaded) {
    const ok = confirm("Файл уже скачан. Перезаписать локальную копию?");
    if (!ok) {
      return;
    }
    overwrite = true;
  }
  await downloadFile(file.id, overwrite);
  await reloadFiles();
}

export async function handleOpenAction({ file, openFile, reloadFiles }: OpenActionArgs): Promise<void> {
  await openFile(file.id);
  await reloadFiles();
}

export async function handleOpenFolderAction({ file, openFileFolder }: OpenFolderActionArgs): Promise<void> {
  await openFileFolder(file.id);
}
