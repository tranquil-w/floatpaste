import test from "node:test";
import assert from "node:assert/strict";
import { resolveTooltipShowPosition } from "../src/features/picker/tooltipState.ts";
import { escapeHtml } from "../src/features/picker/tooltipHtml.ts";

test("tooltip 坐标会把 CSS 像素换算为桌面物理像素", () => {
  const position = resolveTooltipShowPosition({
    activeRequestId: 1,
    requestId: 1,
    outerPosition: { x: 100, y: 200 },
    scaleFactor: 1.5,
    clientPosition: { x: 40, y: 30 },
  });

  assert.deepEqual(position, { x: 178, y: 269 });
});

test("tooltip 请求失效后，迟到返回的定位结果不会继续显示", () => {
  const position = resolveTooltipShowPosition({
    activeRequestId: 2,
    requestId: 1,
    outerPosition: { x: 100, y: 200 },
    scaleFactor: 1,
    clientPosition: { x: 40, y: 30 },
  });

  assert.equal(position, null);
});

test("escapeHtml 转义 HTML 特殊字符", () => {
  assert.equal(escapeHtml('<script>alert("xss")</script>'), "&lt;script&gt;alert(&quot;xss&quot;)&lt;/script&gt;");
  assert.equal(escapeHtml("a & b < c > d \"e\""), "a &amp; b &lt; c &gt; d &quot;e&quot;");
  assert.equal(escapeHtml(""), "");
  assert.equal(escapeHtml("普通文本"), "普通文本");
  assert.equal(escapeHtml("&amp;"), "&amp;amp;");
});
