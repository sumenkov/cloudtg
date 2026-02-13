import React from "react";
import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import type { FileItem } from "../store/app";
import { FileList } from "../components/file-manager/FileList";
import {
  displayFileSizeBytes,
  handleDownloadAction,
  handleOpenAction,
  handleOpenFolderAction,
  shouldShowOpenFolderButton
} from "../components/file-manager/fileActions";

function makeFile(overrides: Partial<FileItem> = {}): FileItem {
  return {
    id: "file-1",
    dir_id: "dir-1",
    name: "report.txt",
    size: 0,
    local_size: null,
    is_downloaded: false,
    hash: "deadbeef",
    tg_chat_id: -100,
    tg_msg_id: 10,
    created_at: 0,
    is_broken: false,
    ...overrides
  };
}

describe("fileActions", () => {
  it("displayFileSizeBytes returns 0 for non-downloaded file", () => {
    expect(displayFileSizeBytes(makeFile())).toBe(0);
  });

  it("displayFileSizeBytes returns local size for downloaded file", () => {
    expect(displayFileSizeBytes(makeFile({ is_downloaded: true, local_size: 2048 }))).toBe(2048);
    expect(displayFileSizeBytes(makeFile({ is_downloaded: true, local_size: null }))).toBe(0);
  });

  it("shouldShowOpenFolderButton follows download state", () => {
    expect(shouldShowOpenFolderButton(makeFile({ is_downloaded: true }))).toBe(true);
    expect(shouldShowOpenFolderButton(makeFile({ is_downloaded: false }))).toBe(false);
  });

  it("download action downloads without overwrite for new file", async () => {
    const downloadFile = vi.fn(async () => "/tmp/report.txt");
    const reloadFiles = vi.fn(async () => {});
    const confirm = vi.fn(() => true);

    await handleDownloadAction({
      file: makeFile({ id: "new-file", is_downloaded: false }),
      confirm,
      downloadFile,
      reloadFiles
    });

    expect(confirm).not.toHaveBeenCalled();
    expect(downloadFile).toHaveBeenCalledWith("new-file", false);
    expect(reloadFiles).toHaveBeenCalledTimes(1);
  });

  it("download action cancels overwrite when user rejects confirm", async () => {
    const downloadFile = vi.fn(async () => "/tmp/report.txt");
    const reloadFiles = vi.fn(async () => {});
    const confirm = vi.fn(() => false);

    await handleDownloadAction({
      file: makeFile({ id: "existing-file", is_downloaded: true }),
      confirm,
      downloadFile,
      reloadFiles
    });

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(downloadFile).not.toHaveBeenCalled();
    expect(reloadFiles).not.toHaveBeenCalled();
  });

  it("download action passes overwrite=true when user confirms", async () => {
    const downloadFile = vi.fn(async () => "/tmp/report.txt");
    const reloadFiles = vi.fn(async () => {});
    const confirm = vi.fn(() => true);

    await handleDownloadAction({
      file: makeFile({ id: "existing-file", is_downloaded: true }),
      confirm,
      downloadFile,
      reloadFiles
    });

    expect(confirm).toHaveBeenCalledTimes(1);
    expect(downloadFile).toHaveBeenCalledWith("existing-file", true);
    expect(reloadFiles).toHaveBeenCalledTimes(1);
  });

  it("open action opens file and reloads list", async () => {
    const openFile = vi.fn(async () => {});
    const reloadFiles = vi.fn(async () => {});

    await handleOpenAction({
      file: makeFile({ id: "open-id" }),
      openFile,
      reloadFiles
    });

    expect(openFile).toHaveBeenCalledWith("open-id");
    expect(reloadFiles).toHaveBeenCalledTimes(1);
  });

  it("open folder action only opens folder", async () => {
    const openFileFolder = vi.fn(async () => {});

    await handleOpenFolderAction({
      file: makeFile({ id: "folder-id" }),
      openFileFolder
    });

    expect(openFileFolder).toHaveBeenCalledWith("folder-id");
  });
});

describe("FileList", () => {
  it("shows open-folder button only for downloaded files", () => {
    const html = renderToStaticMarkup(
      <FileList
        files={[
          makeFile({ id: "a", name: "remote.bin", is_downloaded: false }),
          makeFile({ id: "b", name: "local.bin", is_downloaded: true, local_size: 1024 })
        ]}
        selectedFiles={new Set<string>()}
        downloadingFileIds={new Set<string>()}
        onToggleSelect={() => {}}
        onDownload={() => {}}
        onOpen={() => {}}
        onOpenFolder={() => {}}
        onShare={() => {}}
        onRepair={() => {}}
        onDelete={() => {}}
      />
    );

    expect(html).toContain("remote.bin");
    expect(html).toContain("local.bin");
    const openFolderCount = (html.match(/Открыть папку/g) ?? []).length;
    expect(openFolderCount).toBe(1);
  });

  it("shows real local size after download and 0 B before download", () => {
    const html = renderToStaticMarkup(
      <FileList
        files={[
          makeFile({ id: "a", name: "before.txt", is_downloaded: false, local_size: null }),
          makeFile({ id: "b", name: "after.txt", is_downloaded: true, local_size: 2048 })
        ]}
        selectedFiles={new Set<string>()}
        downloadingFileIds={new Set<string>()}
        onToggleSelect={() => {}}
        onDownload={() => {}}
        onOpen={() => {}}
        onOpenFolder={() => {}}
        onShare={() => {}}
        onRepair={() => {}}
        onDelete={() => {}}
      />
    );

    expect(html).toContain("0 Б");
    expect(html).toContain("2.0 КБ");
  });
});
