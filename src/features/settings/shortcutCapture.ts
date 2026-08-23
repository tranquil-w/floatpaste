export type ShortcutCaptureResult =
  | { kind: "complete"; value: string }
  | { kind: "clear" }
  | { kind: "pending" }
  | { kind: "cancel" }
  | { kind: "invalid"; reason: string };

type ShortcutCaptureEvent = {
  key: string;
  code: string;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
};

const MAIN_KEY_PATTERN = /^(?:Key[A-Z]|Digit[0-9]|F[1-9]|F1[0-2])$/;

function resolveMainKey(event: ShortcutCaptureEvent): { key: string } | { error: string } {
  if (event.code === "Space") {
    return { key: "Space" };
  }
  if (event.code === "Tab") {
    return { key: "Tab" };
  }
  if (event.code === "Comma") {
    return { key: "Comma" };
  }
  if (event.code === "Period") {
    return { key: "Period" };
  }
  if (event.code === "Slash") {
    return { key: "Slash" };
  }
  if (event.code === "ArrowUp" || event.code === "ArrowDown") {
    return { key: event.code };
  }

  if (!MAIN_KEY_PATTERN.test(event.code)) {
    return { error: "不支持的按键，请使用字母、数字或 F1-F12" };
  }

  return { key: event.code.replace(/^(Key|Digit)/, "") };
}

/**
 * 把原始键盘事件解析为可注册的全局快捷键字符串（如 `Ctrl+Alt+P`）。
 * 规则：必须包含 Ctrl/Alt/Win 之一的修饰键（Shift 不能单独作修饰），且有主键；
 * 修饰键顺序固定为 Ctrl → Alt → Shift → Super，与后端注册格式兼容。
 */
export function captureShortcut(event: ShortcutCaptureEvent): ShortcutCaptureResult {
  if (event.key === "Escape") {
    return { kind: "cancel" };
  }

  if (event.key === "Backspace") {
    return { kind: "clear" };
  }

  const mainKey = resolveMainKey(event);
  const hasModifier = event.altKey || event.ctrlKey || event.metaKey;

  // 只按下修饰键时继续等待主键
  if (
    mainKey &&
    "error" in mainKey &&
    (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) &&
    ["Alt", "Control", "Shift", "Meta", "OS"].includes(event.key)
  ) {
    return { kind: "pending" };
  }

  if ("error" in mainKey) {
    return { kind: "invalid", reason: mainKey.error };
  }

  if (!hasModifier) {
    return {
      kind: "invalid",
      reason: "请至少配合 Ctrl、Alt 或 Win 修饰键，避免影响正常打字",
    };
  }

  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.metaKey) {
    parts.push("Super");
  }
  parts.push(mainKey.key);

  return { kind: "complete", value: parts.join("+") };
}
