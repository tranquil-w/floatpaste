// src/shared/queries/clipQueries.ts
import { useMutation, useQuery } from "@tanstack/react-query";
import { getItemDetail, updateTextItem } from "../../bridge/commands.ts";
import { queryClient } from "../../app/queryClient.ts";
import { queryKeys } from "./queryKeys.ts";

// 条目内容低频变化；staleTime 放宽避免上下键导航时反复触发 get_item_detail IPC
const DETAIL_STALE_TIME_MS = 30_000;

export function useItemDetailQuery(id: string | null) {
  return useQuery({
    queryKey: queryKeys.clipDetail(id as string),
    queryFn: () => getItemDetail(id as string),
    enabled: Boolean(id),
    staleTime: DETAIL_STALE_TIME_MS,
  });
}

/** 条目增删改后需要失效的列表与详情缓存，统一在此维护 */
export function invalidateClipQueries() {
  return Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.searchRecents }),
    queryClient.invalidateQueries({ queryKey: queryKeys.searchQueries }),
    queryClient.invalidateQueries({ queryKey: queryKeys.clipDetails }),
    queryClient.invalidateQueries({ queryKey: queryKeys.pickerRecents }),
  ]);
}

export function useUpdateTextMutation() {
  return useMutation({
    mutationFn: ({ id, text }: { id: string; text: string }) => updateTextItem(id, text),
    onSuccess: async (detail) => {
      queryClient.setQueryData(queryKeys.clipDetail(detail.id), detail);
      await invalidateClipQueries();
    },
  });
}
