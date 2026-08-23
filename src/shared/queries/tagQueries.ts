import { useQuery } from "@tanstack/react-query";
import { listTags } from "../../bridge/commands";
import { queryKeys } from "./queryKeys";

/** 标签列表查询的唯一实现：搜索窗口、设置页与编辑器共用同一份缓存 */
export function useTagsQuery(enabled = true) {
  return useQuery({
    queryKey: queryKeys.tags,
    queryFn: () => listTags(),
    enabled,
    staleTime: 0,
  });
}
