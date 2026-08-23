import { useEffect, useRef, useState } from "react";
import { captureShortcut } from "./shortcutCapture";

const KBD_BADGE =
  "inline-flex h-[22px] min-w-[22px] items-center justify-center rounded-[4px] border border-pg-border-default bg-pg-canvas-default px-1.5 font-mono text-[11px] font-semibold text-pg-fg-default";

const DISPLAY_NAME_BY_PART: Record<string, string> = {
  Super: "Win",
  Ctrl: "Ctrl",
  Alt: "Alt",
  Shift: "Shift",
};

const NOTICE_TIMEOUT_MS = 1800;

type ShortcutInputProps = {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  hint?: string;
};

/** 快捷键录制输入框：点击进入录制态后按下组合键捕获，替代手敲 "Alt+Q" 字符串。 */
export function ShortcutInput({ value, onChange, disabled = false, hint }: ShortcutInputProps) {
  const [recording, setRecording] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const noticeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    return () => {
      if (noticeTimerRef.current) {
        clearTimeout(noticeTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!recording) {
      return;
    }

    const showNotice = (message: string) => {
      setNotice(message);
      if (noticeTimerRef.current) {
        clearTimeout(noticeTimerRef.current);
      }
      noticeTimerRef.current = setTimeout(() => {
        setNotice(null);
      }, NOTICE_TIMEOUT_MS);
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      // 录制态拦截一切按键（包括浏览器/系统组合），交给解析器判断
      event.preventDefault();
      event.stopPropagation();

      const result = captureShortcut(event);
      if (result.kind === "pending") {
        return;
      }

      setRecording(false);
      if (result.kind === "complete") {
        onChange(result.value);
        setNotice(null);
        return;
      }
      if (result.kind === "clear") {
        onChange("");
        setNotice(null);
        return;
      }
      if (result.kind === "invalid") {
        showNotice(result.reason);
      }
    };

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [recording, onChange]);

  useEffect(() => {
    if (!recording) {
      return;
    }

    const handlePointerDown = (event: globalThis.MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        setRecording(false);
        setNotice(null);
      }
    };

    document.addEventListener("mousedown", handlePointerDown);
    return () => document.removeEventListener("mousedown", handlePointerDown);
  }, [recording]);

  return (
    <div ref={rootRef}>
      <button
        aria-label={recording ? "正在录制快捷键" : "录制快捷键"}
        className={`flex h-[42px] w-full items-center gap-1.5 rounded-xl border px-4 text-left text-sm transition-colors ${
          recording
            ? "border-pg-accent-fg bg-pg-canvas-default ring-1 ring-pg-accent-fg"
            : "border-pg-border-default bg-pg-canvas-default hover:border-pg-accent-fg"
        } ${disabled ? "cursor-not-allowed opacity-60" : ""}`}
        disabled={disabled}
        onClick={() => {
          setNotice(null);
          setRecording((current) => !current);
        }}
        type="button"
      >
        {recording ? (
          <span className="text-[13px] text-pg-accent-fg">按下新的组合键，Esc 取消，Backspace 清除</span>
        ) : value ? (
          value.split("+").map((part) => (
            <kbd className={KBD_BADGE} key={part}>
              {DISPLAY_NAME_BY_PART[part] ?? part}
            </kbd>
          ))
        ) : (
          <span className="text-pg-fg-subtle">未设置，点击后按下组合键</span>
        )}
      </button>
      {notice ? (
        <p className="mt-1.5 text-xs leading-relaxed text-pg-danger-fg">{notice}</p>
      ) : hint ? (
        <p className="mt-1.5 text-xs leading-relaxed text-pg-fg-subtle">{hint}</p>
      ) : null}
    </div>
  );
}
