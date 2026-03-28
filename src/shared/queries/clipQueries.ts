// src/shared/queries/clipQueries.ts
import { useMutation, useQuery } from "@tanstack/react-query";
import { getItemDetail, updateTextItem } from "../../bridge/commands";
import { queryClient } from "../../app/queryClient";

export function useItemDetailQuery(id: string | null) {
  return useQuery({
    queryKey: ["detail", id],
    queryFn: () => getItemDetail(id as string),
    enabled: Boolean(id),
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
