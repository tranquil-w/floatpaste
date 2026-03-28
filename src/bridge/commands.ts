import { invoke } from "@tauri-apps/api/core";
import type {
  ClipItemDetail,
  ClipItemSummary,
  PasteOption,
  PasteResult,
  SearchQuery,
  SearchResult,
} from "../shared/types/clips";
import type { UserSetting } from "../shared/types/settings";
import {
  mockDeleteItem,
  mockGetItemDetail,
  mockGetSettings,
  mockListFavoriteItems,
  mockListRecentItems,
  mockPasteItem,
  mockPauseMonitoring,
  mockResumeMonitoring,
  mockSearchItems,
  mockSetItemFavorited,
  mockUpdateSettings,
  mockUpdateTextItem,
} from "./mockBackend";
import { isTauriRuntime } from "./runtime";

export async function listRecentItems(limit: number): Promise<ClipItemSummary[]> {
  if (!isTauriRuntime()) {
    return mockListRecentItems(limit);
  }
  return invoke("list_recent_items", { limit });
}

export async function listFavoriteItems(limit: number): Promise<ClipItemSummary[]> {
  if (!isTauriRuntime()) {
    return mockListFavoriteItems(limit);
  }
  return invoke("list_favorite_items", { limit });
}

export async function searchItems(query: SearchQuery): Promise<SearchResult> {
  if (!isTauriRuntime()) {
    return mockSearchItems(query);
  }
  return invoke("search_items", { query });
}

export async function getItemDetail(id: string): Promise<ClipItemDetail> {
  if (!isTauriRuntime()) {
    return mockGetItemDetail(id);
  }
  return invoke("get_item_detail", { id });
}

export async function updateTextItem(id: string, text: string): Promise<ClipItemDetail> {
  if (!isTauriRuntime()) {
    return mockUpdateTextItem(id, text);
  }
  return invoke("update_text_item", { id, text });
}

export async function deleteItem(id: string): Promise<void> {
  if (!isTauriRuntime()) {
    return mockDeleteItem(id);
  }
  return invoke("delete_item", { id });
}

export async function setItemFavorited(id: string, value: boolean): Promise<void> {
  if (!isTauriRuntime()) {
    return mockSetItemFavorited(id, value);
  }
  return invoke("set_item_favorited", { id, value });
}

export async function pasteItem(id: string, option: PasteOption): Promise<PasteResult> {
  if (!isTauriRuntime()) {
    return mockPasteItem(id, option);
  }
  return invoke("paste_item", { id, option });
}

export async function getSettings(): Promise<UserSetting> {
  if (!isTauriRuntime()) {
    return mockGetSettings();
  }
  return invoke("get_settings");
}

export async function updateSettings(payload: UserSetting): Promise<UserSetting> {
  if (!isTauriRuntime()) {
    return mockUpdateSettings(payload);
  }
  return invoke("update_settings", { payload });
}

export async function pauseMonitoring(): Promise<UserSetting> {
  if (!isTauriRuntime()) {
    return mockPauseMonitoring();
  }
  return invoke("pause_monitoring");
}

export async function resumeMonitoring(): Promise<UserSetting> {
  if (!isTauriRuntime()) {
    return mockResumeMonitoring();
  }
  return invoke("resume_monitoring");
}

export async function showPicker(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("show_picker_from_manager");
}

export async function hidePicker(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("hide_picker");
}

export async function openManager(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("open_manager");
}

export async function openEditorFromPicker(itemId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("open_editor_from_picker", { itemId });
}

export async function openEditorFromWorkbench(itemId: string): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("open_editor_from_workbench", { itemId });
}

export async function hideEditor(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("hide_editor");
}

export async function openWorkbenchGlobal(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("open_workbench_global");
}

export async function hideWorkbench(): Promise<void> {
  if (!isTauriRuntime()) {
    return;
  }
  return invoke("hide_workbench");
}
