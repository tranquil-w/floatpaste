import test from "node:test";
import assert from "node:assert/strict";
import { getEditorKeyboardAction } from "../src/features/editor/keyboard.ts";

test("编辑窗口在未弹出确认框时仍保留 Esc 请求关闭", () => {
  const action = getEditorKeyboardAction({
    key: "Escape",
    ctrlKey: false,
    metaKey: false,
    closeConfirmOpen: false,
  });

  assert.equal(action, "request-close");
});

test("编辑窗口在未弹出确认框时支持 Ctrl+S 保存", () => {
  const action = getEditorKeyboardAction({
    key: "s",
    ctrlKey: true,
    metaKey: false,
    closeConfirmOpen: false,
  });

  assert.equal(action, "save");
});

test("未保存确认框打开后，Esc 应取消弹窗，Enter 应触发主操作", () => {
  const cancelAction = getEditorKeyboardAction({
    key: "Escape",
    ctrlKey: false,
    metaKey: false,
    closeConfirmOpen: true,
  });
  const primaryAction = getEditorKeyboardAction({
    key: "Enter",
    ctrlKey: false,
    metaKey: false,
    closeConfirmOpen: true,
  });

  assert.equal(cancelAction, "confirm-cancel");
  assert.equal(primaryAction, "confirm-primary");
});

