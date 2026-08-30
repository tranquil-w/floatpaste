import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { useQueryClient } from "@tanstack/react-query";
import { setItemTags } from "../../bridge/commands";
import { getErrorMessage } from "../../shared/utils/error";
import { invalidateClipQueries } from "../../shared/queries/clipQueries";
import { queryKeys } from "../../shared/queries/queryKeys";
import { useTagsQuery } from "../../shared/queries/tagQueries";
import { buildTagSuggestions, normalizeTagInput } from "./tagSuggestions";
import type { TagSuggestion } from "./tagSuggestions";

const MAX_TAGS_PER_ITEM = 20;
const MAX_TAG_NAME_LEN = 32;

interface TagEditorProps {
  itemId: string;
  tags: string[];
  onError: (message: string) => void;
}

/**
 * 条目标签编辑器：统一输入容器（芯片内嵌）+ 建议浮层。
 * 添加/移除即时提交（失败回滚）；轮廓只在容器上，芯片与输入自身不带边框。
 */
export function TagEditor({ itemId, tags, onError }: TagEditorProps) {
  const queryClient = useQueryClient();
  const [draftTags, setDraftTags] = useState<string[] | null>(null);
  const [input, setInput] = useState("");
  const [focused, setFocused] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const current = draftTags ?? tags;

  const tagsQuery = useTagsQuery();

  const suggestions = useMemo(
    () => buildTagSuggestions({ allTags: tagsQuery.data ?? [], current, keyword: input }),
    [current, input, tagsQuery.data],
  );
  const open = focused && suggestions.length > 0;
  const activeIndex = Math.min(highlight, suggestions.length - 1);
  // Esc 的本地消费范围：有输入内容，或建议浮层正开着
  const consumesEscape = Boolean(normalizeTagInput(input)) || open;

  useEffect(() => {
    if (
      draftTags &&
      draftTags.length === tags.length &&
      draftTags.every((name, i) => name === tags[i])
    ) {
      // 乐观值已被服务器数据接管，交还控制权以订阅后续外部变更
      setDraftTags(null);
    }
  }, [draftTags, tags]);

  useEffect(() => {
    if (!open) {
      return;
    }
    listRef.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }, [activeIndex, open]);

  async function commitTags(next: string[]) {
    const previous = current;
    setDraftTags(next);
    try {
      await setItemTags(itemId, next);
      // 详情与列表都携带标签信息，一并失效避免编辑器外的列表/详情显示陈旧标签
      await queryClient.invalidateQueries({ queryKey: queryKeys.tags });
      await invalidateClipQueries();
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

  function adoptSuggestion(suggestion: TagSuggestion) {
    if (suggestion.kind === "exists") {
      // 重复标签：仅清空输入，不重复提交
      setInput("");
      return;
    }
    addTag(suggestion.name);
  }

  function handleInputKeyDown(event: ReactKeyboardEvent<HTMLInputElement>) {
    if (event.nativeEvent.isComposing) {
      return;
    }
    if (open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      setHighlight((activeIndex + delta + suggestions.length) % suggestions.length);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      if (open && suggestions[activeIndex]) {
        adoptSuggestion(suggestions[activeIndex]);
      } else {
        addTag(input);
      }
      return;
    }
    if (event.key === "Tab" && open && suggestions[activeIndex]) {
      const suggestion = suggestions[activeIndex];
      // Tab 仅补全文本（match/create），让用户在此基础上继续编辑
      if (suggestion.kind !== "exists") {
        event.preventDefault();
        setInput(suggestion.name);
        setHighlight(0);
      }
      return;
    }
    if (event.key === "Escape" && consumesEscape) {
      event.preventDefault();
      event.stopPropagation();
      setInput("");
      event.currentTarget.blur();
      return;
    }
    if (event.key === "Backspace" && input === "" && current.length > 0) {
      removeTag(current[current.length - 1] ?? "");
    }
  }

  return (
    <section aria-label="条目标签" className="relative shrink-0">
      <div
        className="flex min-h-[38px] cursor-text flex-wrap items-center gap-1.5 rounded-lg border border-transparent bg-pg-canvas-subtle px-2 py-1.5 transition-colors focus-within:border-pg-border-accent focus-within:bg-pg-canvas-inset"
        onClick={(event) => {
          if (event.target === event.currentTarget) {
            inputRef.current?.focus();
          }
        }}
      >
        {current.map((name) => (
          <span
            className="flex h-6 shrink-0 items-center gap-1 rounded-full bg-pg-canvas-default px-2.5 text-[12px] leading-4 text-pg-fg-default"
            key={name}
          >
            {name}
            <button
              aria-label={`移除标签 ${name}`}
              className="-mr-1 text-pg-fg-subtle transition-colors hover:text-pg-danger-fg"
              onClick={() => removeTag(name)}
              onMouseDown={(event) => event.preventDefault()}
              type="button"
            >
              ×
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          aria-activedescendant={open ? `tag-editor-option-${activeIndex}` : undefined}
          aria-autocomplete="list"
          aria-controls={open ? "tag-editor-suggestions" : undefined}
          aria-expanded={open}
          aria-label="添加标签"
          className="h-6 min-w-[7rem] flex-1 border-0 bg-transparent p-0 text-[13px] leading-5 text-pg-fg-default outline-none placeholder:text-pg-fg-subtle focus:outline-none"
          data-esc-scope={consumesEscape ? "local" : undefined}
          onBlur={() => setFocused(false)}
          onChange={(event) => {
            setInput(event.target.value);
            setHighlight(0);
          }}
          onFocus={() => {
            setFocused(true);
            setHighlight(0);
          }}
          onKeyDown={handleInputKeyDown}
          placeholder={current.length > 0 ? "添加标签…" : "输入标签，回车添加"}
          role="combobox"
          value={input}
        />
      </div>
      {open ? (
        <div
          className="absolute left-0 top-full z-20 mt-1 w-72 max-w-full overflow-hidden rounded-md border border-pg-border-subtle bg-pg-canvas-default shadow-pg-md"
          id="tag-editor-suggestions"
          ref={listRef}
          role="listbox"
        >
          <div className="max-h-56 overflow-y-auto p-1">
            {suggestions.map((suggestion, index) => {
              const isSelected = index === activeIndex;
              const baseClass = `flex cursor-default items-center justify-between gap-2 rounded-md px-2 py-1.5 text-[13px] leading-5 transition-colors ${
                isSelected ? "bg-pg-canvas-subtle" : ""
              }`;
              return (
                <div
                  aria-disabled={suggestion.kind === "exists" || undefined}
                  aria-selected={isSelected}
                  className={`${baseClass} ${
                    suggestion.kind === "exists" ? "text-pg-fg-subtle" : "text-pg-fg-default"
                  }`}
                  id={`tag-editor-option-${index}`}
                  key={`${suggestion.kind}-${suggestion.name}`}
                  onClick={() => adoptSuggestion(suggestion)}
                  onMouseDown={(event) => event.preventDefault()}
                  role="option"
                >
                  {suggestion.kind === "match" ? (
                    <>
                      <HighlightMatch name={suggestion.name} keyword={input} />
                      <span className="shrink-0 text-[11px] leading-4 text-pg-fg-subtle">
                        {suggestion.itemCount} 条
                      </span>
                    </>
                  ) : suggestion.kind === "create" ? (
                    <span className="text-pg-accent-fg">创建标签「{suggestion.name}」</span>
                  ) : (
                    <span>「{suggestion.name}」已添加</span>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ) : null}
    </section>
  );
}

/** 命中片段用强调色标出 */
function HighlightMatch({ name, keyword }: { name: string; keyword: string }) {
  const kw = normalizeTagInput(keyword).toLowerCase();
  const index = kw ? name.toLowerCase().indexOf(kw) : -1;
  if (index === -1) {
    return <span>{name}</span>;
  }
  return (
    <span>
      {name.slice(0, index)}
      <span className="font-medium text-pg-accent-fg">{name.slice(index, index + kw.length)}</span>
      {name.slice(index + kw.length)}
    </span>
  );
}
