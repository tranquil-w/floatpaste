import { useMutation, useQuery } from "@tanstack/react-query";
import {
  deleteItem,
  getItemDetail,
  getSettings,
  listFavoriteItems,
  pasteItem,
  searchItems,
  setItemFavorited,
  updateSettings,
  updateTextItem,
} from "../../bridge/commands";
import { queryClient } from "../../app/queryClient";
import type { PasteOption, SearchQuery } from "../../shared/types/clips";
import type { UserSetting } from "../../shared/types/settings";

export function useFavoritesQuery(limit = 8) {
  return useQuery({
    queryKey: ["favorites", limit],
    queryFn: () => listFavoriteItems(limit),
  });
}

export function useSearchQuery(query: SearchQuery) {
  return useQuery({
    queryKey: ["search", query],
    queryFn: () => searchItems(query),
  });
}

export function useItemDetailQuery(id: string | null) {
  return useQuery({
    queryKey: ["detail", id],
    queryFn: () => getItemDetail(id as string),
    enabled: Boolean(id),
  });
}

export function useSettingsQuery() {
  return useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });
}

function invalidateClipQueries() {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: ["favorites"] }),
    queryClient.invalidateQueries({ queryKey: ["search"] }),
    queryClient.invalidateQueries({ queryKey: ["detail"] }),
    queryClient.invalidateQueries({ queryKey: ["picker-recent"] }),
    queryClient.invalidateQueries({ queryKey: ["workbench-recent"] }),
    queryClient.invalidateQueries({ queryKey: ["workbench-search"] }),
  ]);
}

export function useUpdateTextMutation() {
  return useMutation({
    mutationFn: ({ id, text }: { id: string; text: string }) => updateTextItem(id, text),
    onSuccess: async (detail) => {
      queryClient.setQueryData(["detail", detail.id], detail);
      await invalidateClipQueries();
    },
  });
}

export function useDeleteItemMutation() {
  return useMutation({
    mutationFn: (id: string) => deleteItem(id),
    onSuccess: invalidateClipQueries,
  });
}

export function useSetFavoritedMutation() {
  return useMutation({
    mutationFn: ({ id, value }: { id: string; value: boolean }) => setItemFavorited(id, value),
    onSuccess: invalidateClipQueries,
  });
}

export function usePasteMutation() {
  return useMutation({
    mutationFn: ({ id, option }: { id: string; option: PasteOption }) => pasteItem(id, option),
    onSuccess: invalidateClipQueries,
  });
}

export function useUpdateSettingsMutation() {
  return useMutation({
    mutationFn: (payload: UserSetting) => updateSettings(payload),
    onSuccess: (nextValue) => {
      queryClient.setQueryData(["settings"], nextValue);
    },
  });
}
