import test from "node:test";
import assert from "node:assert/strict";
import { getWorkbenchKeyboardAction } from "../src/features/workbench/keyboard.ts";

test("搜索窗口在激活状态下支持方向键导航与确认快捷键", () => {
  assert.equal(
    getWorkbenchKeyboardAction({
      key: "ArrowUp",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "navigate-up",
  );
  assert.equal(
    getWorkbenchKeyboardAction({
      key: "ArrowDown",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "navigate-down",
  );
  assert.equal(
    getWorkbenchKeyboardAction({
      key: "Enter",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "paste",
  );
});

test("搜索窗口支持 Ctrl+Enter 编辑与 Escape 关闭", () => {
  assert.equal(
    getWorkbenchKeyboardAction({
      key: "Enter",
      ctrlKey: true,
      metaKey: false,
      inputSuspended: false,
    }),
    "edit-item",
  );
  assert.equal(
    getWorkbenchKeyboardAction({
      key: "Escape",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "close",
  );
});

test("当 Picker 覆盖在搜索窗口上方时，搜索窗口应暂停自己的本地快捷键", () => {
  assert.equal(
    getWorkbenchKeyboardAction({
      key: "ArrowDown",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: true,
    }),
    null,
  );
});

