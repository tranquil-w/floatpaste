import test from "node:test";
import assert from "node:assert/strict";
import { shouldPreventSearchItemMouseFocus } from "../src/features/search/itemPointer.ts";

test("搜索结果项应阻止主键点击抢走焦点，避免留下浏览器默认蓝色焦点框", () => {
  assert.equal(shouldPreventSearchItemMouseFocus(0), true);
});

test("搜索结果项不应拦截非主键按下", () => {
  assert.equal(shouldPreventSearchItemMouseFocus(1), false);
  assert.equal(shouldPreventSearchItemMouseFocus(2), false);
});
