import test from "node:test";
import assert from "node:assert/strict";
import { resolveTooltipShowPosition } from "../src/features/picker/tooltipState.ts";
import { buildTooltipHtml, escapeHtml, escapeHtmlAttribute } from "../src/features/picker/tooltipHtml.ts";

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

test("图片 tooltip 会转义属性值并携带 requestId", () => {
  const html = buildTooltipHtml(
    {
      id: "demo-3",
      type: "image",
      contentPreview: '图片 <预览>',
      tooltipText: null,
      sourceApp: '微信 "桌面端"',
      isFavorited: false,
      fileCount: 0,
      directoryCount: 0,
      createdAt: "2026-04-02T10:00:00.000Z",
      updatedAt: "2026-04-02T10:00:00.000Z",
      lastUsedAt: null,
      imagePath: "images/demo-3.png",
      imageWidth: 1920,
      imageHeight: 1080,
      imageFormat: "png",
      fileSize: 2400000,
    },
    {
      imageUrl: 'asset://"demo"&preview=<bad>',
      requestId: 7,
    },
  );

  assert.match(html, /<img[^>]+src="asset:\/\/&quot;demo&quot;&amp;preview=&lt;bad&gt;"/);
  assert.match(html, /data-request-id="7"/);
  assert.match(html, /meta-size">1920 × 1080</);
});

test("图片 tooltip 在没有图片 URL 时回退为纯文本内容", () => {
  const html = buildTooltipHtml(
    {
      id: "demo-3",
      type: "image",
      contentPreview: "图片 (1920 × 1080, 2.4 MB)",
      tooltipText: "图片预览暂不可用",
      sourceApp: "微信",
      isFavorited: false,
      fileCount: 0,
      directoryCount: 0,
      createdAt: "2026-04-02T10:00:00.000Z",
      updatedAt: "2026-04-02T10:00:00.000Z",
      lastUsedAt: null,
      imagePath: "images/demo-3.png",
      imageWidth: 1920,
      imageHeight: 1080,
      imageFormat: "png",
      fileSize: 2400000,
    },
    {
      imageUrl: null,
      requestId: 8,
    },
  );

  assert.match(html, /tooltip-content">图片预览暂不可用</);
  assert.doesNotMatch(html, /<img/);
});

test("escapeHtmlAttribute 会转义属性敏感字符", () => {
  assert.equal(escapeHtmlAttribute('"quoted"&<tag>'), "&quot;quoted&quot;&amp;&lt;tag&gt;");
});
