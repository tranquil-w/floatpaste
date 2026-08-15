import { useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { listTags, setItemTags } from "../../bridge/commands";
import { getErrorMessage } from "../../shared/utils/error";

const MAX_TAGS_PER_ITEM = 20;
const MAX_TAG_NAME_LEN = 32;

export function normalizeTagInput(value: string): string {
  return value.split(/\s+/).join(" ").trim();
}

interface TagEditorProps {
  itemId: string;
  tags: string[];
  onError: (message: string) => void;
}

/** 条目标签编辑器：芯片 + 输入 + 建议，添加/移除即时提交（失败回滚）。 */
export function TagEditor({ itemId, tags, onError }: TagEditorProps) {
  const queryClient = useQueryClient();
  const [draftTags, setDraftTags] = useState<string[] | null>(null);
  const [input, setInput] = useState("");
  const current = draftTags ?? tags;

  const tagsQuery = useQuery({
    queryKey: ["tags"],
    queryFn: () => listTags(),
    staleTime: 0,
  });

  const suggestions = useMemo(() => {
    const keyword = input.trim().toLowerCase();
    return (tagsQuery.data ?? [])
      .filter((tag) => !current.some((name) => name.toLowerCase() === tag.name.toLowerCase()))
      .filter((tag) => !keyword || tag.name.toLowerCase().startsWith(keyword))
      .slice(0, 6);
  }, [current, input, tagsQuery.data]);

  async function commitTags(next: string[]) {
    const previous = current;
    setDraftTags(next);
    try {
      await setItemTags(itemId, next);
      await queryClient.invalidateQueries({ queryKey: ["tags"] });
    } catch (error) {
      setDraftTags(previous);
      onError(`标签保存失败：${getErrorMessage(error, "请稍后重试")}`);
    }
  }

  function addTag(rawName: string) {
    const name = normalizeTagInput(rawName);
    if (!name) {
      return;
    }
    if (name.length > MAX_TAG_NAME_LEN) {
      onError(`标签名不能超过 ${MAX_TAG_NAME_LEN} 个字符`);
      return;
    }
    if (current.some((existing) => existing.toLowerCase() === name.toLowerCase())) {
      setInput("");
      return;
    }
    if (current.length >= MAX_TAGS_PER_ITEM) {
      onError(`单条目标签数不能超过 ${MAX_TAGS_PER_ITEM} 个`);
      return;
    }
    setInput("");
    void commitTags([...current, name]);
  }

  function removeTag(name: string) {
    void commitTags(current.filter((existing) => existing !== name));
  }

  return (
    <section aria-label="条目标签" className="shrink-0">
      <div className="flex flex-wrap items-center gap-1.5">
        {current.map((name) => (
          <span
            className="flex items-center gap-1 rounded-full bg-pg-canvas-subtle px-2.5 py-1 text-[12px] leading-4 text-pg-fg-default"
            key={name}
          >
            {name}
            <button
              aria-label={`移除标签 ${name}`}
              className="text-pg-fg-subtle transition-colors hover:text-pg-danger-fg"
              onClick={() => removeTag(name)}
              type="button"
            >
              ×
            </button>
          </span>
        ))}
        <input
          aria-label="添加标签"
          className="h-7 min-w-[120px] flex-1 rounded-md border border-pg-border-subtle bg-pg-canvas-default px-2 text-[13px] leading-5 text-pg-fg-default outline-none transition-colors placeholder:text-pg-fg-subtle focus:border-pg-accent-fg"
          onChange={(event) => setInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              event.preventDefault();
              addTag(input);
              return;
            }
            if (event.key === "Backspace" && input === "" && current.length > 0) {
              removeTag(current[current.length - 1] ?? "");
            }
          }}
          placeholder="输入标签后回车添加"
          value={input}
        />
      </div>
      {input.trim() && suggestions.length > 0 ? (
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          {suggestions.map((tag) => (
            <button
              className="rounded-full border border-pg-border-subtle px-2.5 py-1 text-[12px] leading-4 text-pg-fg-muted transition-colors hover:bg-pg-canvas-subtle"
              key={tag.name}
              onClick={() => addTag(tag.name)}
              type="button"
            >
              {tag.name}
            </button>
          ))}
        </div>
      ) : null}
    </section>
  );
}
