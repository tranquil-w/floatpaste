import test from "node:test";
import assert from "node:assert/strict";
import { getSearchKeyboardAction } from "../src/features/search/keyboard.ts";

test("搜索窗口在激活状态下支持方向键导航与确认快捷键", () => {
  assert.equal(
    getSearchKeyboardAction({
      key: "ArrowUp",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "navigate-up",
  );
  assert.equal(
    getSearchKeyboardAction({
      key: "ArrowDown",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "navigate-down",
  );
  assert.equal(
    getSearchKeyboardAction({
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
    getSearchKeyboardAction({
      key: "Enter",
      ctrlKey: true,
      metaKey: false,
      inputSuspended: false,
    }),
    "edit-item",
  );
  assert.equal(
    getSearchKeyboardAction({
      key: "Escape",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    "close",
  );
});

test("搜索窗口使用 Ctrl+Space 切换收藏，裸 Space 不触发收藏", () => {
  assert.equal(
    getSearchKeyboardAction({
      key: " ",
      ctrlKey: true,
      metaKey: false,
      inputSuspended: false,
    }),
    "toggle-favorite",
  );
  assert.equal(
    getSearchKeyboardAction({
      key: " ",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: false,
    }),
    null,
  );
});

test("当 Picker 覆盖在搜索窗口上方时，搜索窗口应暂停自己的本地快捷键", () => {
  assert.equal(
    getSearchKeyboardAction({
      key: "ArrowDown",
      ctrlKey: false,
      metaKey: false,
      inputSuspended: true,
    }),
    null,
  );
});
