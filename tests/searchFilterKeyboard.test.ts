import test from "node:test";
import assert from "node:assert/strict";
import {
  getSearchFilterCommitFocusTarget,
  getSearchFilterOptionAction,
  getSearchFilterTriggerAction,
} from "../src/features/search/filterKeyboard.ts";

test("筛选触发器仅对无修饰键的空格和回车作出响应", () => {
  assert.equal(
    getSearchFilterTriggerAction({
      key: " ",
      ctrlKey: false,
      metaKey: false,
    }),
    "toggle-menu",
  );

  assert.equal(
    getSearchFilterTriggerAction({
      key: " ",
      ctrlKey: true,
      metaKey: false,
    }),
    null,
  );
});

test("筛选选项仅对无修饰键的空格和回车作出提交响应", () => {
  assert.equal(
    getSearchFilterOptionAction({
      key: "Enter",
      ctrlKey: false,
      metaKey: false,
    }),
    "commit",
  );

  assert.equal(
    getSearchFilterOptionAction({
      key: " ",
      ctrlKey: true,
      metaKey: false,
    }),
    null,
  );
});

test("提交筛选后焦点应回到搜索输入框，而不是留在筛选按钮上", () => {
  assert.equal(getSearchFilterCommitFocusTarget(), "search-input");
});
