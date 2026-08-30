import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { deleteTag, renameTag } from "../../bridge/commands";
import { TAGS_CHANGED_EVENT } from "../../bridge/events";
import { getErrorMessage } from "../../shared/utils/error";
import { queryKeys } from "../../shared/queries/queryKeys";
import { useTagsQuery } from "../../shared/queries/tagQueries";
import { useAppEvent } from "../../shared/hooks/useAppEvent";
import { SettingsSection } from "./SettingsSection";
import type { SettingsSectionId } from "./settingsSections";

type Props = {
  registerSection: (id: SettingsSectionId, element: HTMLElement | null) => void;
};

const FORM_INPUT =
  "rounded-xl border border-pg-border-default bg-pg-canvas-default px-3 py-1.5 text-sm outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg focus:ring-1 focus:ring-pg-accent-fg focus-visible:outline-none";

export function TagsSection({ registerSection }: Props) {
  const queryClient = useQueryClient();
  const tagsQuery = useTagsQuery();
  const [renamingTag, setRenamingTag] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [deleteCandidate, setDeleteCandidate] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  useAppEvent(TAGS_CHANGED_EVENT, async () => {
    await queryClient.invalidateQueries({ queryKey: queryKeys.tags });
  });

  const tags = tagsQuery.data ?? [];

  async function refreshTags() {
    await queryClient.invalidateQueries({ queryKey: queryKeys.tags });
  }

  function startRename(name: string) {
    setRenamingTag(name);
    setRenameDraft(name);
    setErrorMessage(null);
  }

  async function commitRename(oldName: string) {
    const newName = renameDraft.trim();
    if (!newName || newName === oldName) {
      setRenamingTag(null);
      return;
    }

    try {
      await renameTag(oldName, newName);
      setRenamingTag(null);
      setErrorMessage(null);
      await refreshTags();
    } catch (error) {
      setErrorMessage(`重命名失败：${getErrorMessage(error, "请稍后重试")}`);
    }
  }

  async function confirmDelete(name: string) {
    try {
      await deleteTag(name);
      setDeleteCandidate(null);
      setErrorMessage(null);
      await refreshTags();
    } catch (error) {
      setErrorMessage(`删除失败：${getErrorMessage(error, "请稍后重试")}`);
    }
  }

  // 新名与其他标签冲突（忽略大小写）时提示将合并
  const renameConflict =
    renamingTag !== null &&
    renameDraft.trim() !== "" &&
    renameDraft.trim() !== renamingTag &&
    tags.some((tag) => tag.name.toLowerCase() === renameDraft.trim().toLowerCase());

  return (
    <SettingsSection
      description="管理剪贴记录标签：重命名、合并与删除。"
      id="tags"
      registerSection={registerSection}
      title="标签"
    >
      {errorMessage ? (
        <div className="rounded-xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-4 py-3 text-sm text-pg-danger-fg">
          {errorMessage}
        </div>
      ) : null}

      {tagsQuery.isLoading ? (
        <p className="text-sm text-pg-fg-subtle">正在加载标签...</p>
      ) : tags.length === 0 ? (
        <p className="text-sm text-pg-fg-subtle">
          暂无标签，可在编辑窗口为条目添加标签后回到这里管理。
        </p>
      ) : (
        <div className="overflow-hidden rounded-xl border border-pg-border-muted bg-pg-canvas-subtle">
          {tags.map((tag) => (
            <div
              className="flex flex-wrap items-center gap-3 border-b border-pg-border-subtle px-4 py-2 last:border-b-0"
              key={tag.name}
            >
              {renamingTag === tag.name ? (
                <>
                  <input
                    autoFocus
                    className={`${FORM_INPUT} min-w-0 flex-1`}
                    onChange={(event) => setRenameDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        void commitRename(tag.name);
                      }
                      if (event.key === "Escape") {
                        setRenamingTag(null);
                      }
                    }}
                    value={renameDraft}
                  />
                  {renameConflict ? (
                    <span className="text-xs text-pg-fg-subtle">与现有标签同名，保存后将合并</span>
                  ) : null}
                  <button
                    className="rounded-lg bg-pg-accent-emphasis px-3 py-1.5 text-xs font-semibold text-pg-fg-on-emphasis"
                    onClick={() => void commitRename(tag.name)}
                    type="button"
                  >
                    保存
                  </button>
                  <button
                    className="rounded-lg border border-pg-border-default px-3 py-1.5 text-xs text-pg-fg-muted"
                    onClick={() => setRenamingTag(null)}
                    type="button"
                  >
                    取消
                  </button>
                </>
              ) : deleteCandidate === tag.name ? (
                <>
                  <span className="flex-1 text-sm text-pg-fg-default">
                    确认删除标签「{tag.name}」？其全部关联将一并移除。
                  </span>
                  <button
                    className="rounded-lg border border-pg-danger-fg/40 px-3 py-1.5 text-xs font-medium text-pg-danger-fg"
                    onClick={() => void confirmDelete(tag.name)}
                    type="button"
                  >
                    确认删除
                  </button>
                  <button
                    className="rounded-lg border border-pg-border-default px-3 py-1.5 text-xs text-pg-fg-muted"
                    onClick={() => setDeleteCandidate(null)}
                    type="button"
                  >
                    取消
                  </button>
                </>
              ) : (
                <>
                  <span className="flex-1 truncate text-sm font-medium text-pg-fg-default">
                    {tag.name}
                  </span>
                  <span className="text-xs text-pg-fg-subtle">{tag.itemCount} 条记录</span>
                  <button
                    className="rounded-lg border border-pg-border-default px-3 py-1.5 text-xs text-pg-fg-muted transition-colors hover:bg-pg-canvas-default hover:text-pg-fg-default"
                    onClick={() => startRename(tag.name)}
                    type="button"
                  >
                    重命名
                  </button>
                  <button
                    className="rounded-lg border border-pg-border-default px-3 py-1.5 text-xs text-pg-fg-muted transition-colors hover:bg-pg-canvas-default hover:text-pg-danger-fg"
                    onClick={() => setDeleteCandidate(tag.name)}
                    type="button"
                  >
                    删除
                  </button>
                </>
              )}
            </div>
          ))}
        </div>
      )}
    </SettingsSection>
  );
}
