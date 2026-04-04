import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { resolveTooltipShowPosition } from "../src/features/picker/tooltipState.ts";
import {
  buildTooltipHtml,
  escapeHtml,
  escapeHtmlAttribute,
  TOOLTIP_IMAGE_PREVIEW_MAX_HEIGHT,
  TOOLTIP_IMAGE_PREVIEW_MAX_WIDTH,
} from "../src/features/picker/tooltipHtml.ts";
import {
  PICKER_IMAGE_THUMBNAIL_SIZE,
} from "../src/features/picker/previewLayout.ts";

function extractTooltipScript(): string {
  const html = fs.readFileSync(path.resolve("public/tooltip.html"), "utf8");
  const match = html.match(/<script>([\s\S]*?)<\/script>/);
  assert.ok(match, "tooltip.html 应包含内联脚本");
  return match[1];
}

type FakeNode = {
  className: string;
  textContent: string;
};

class FakeImageElement {
  dataset: Record<string, string>;
  complete = false;
  naturalWidth = 0;

  private listeners = new Map<string, Set<() => void>>();

  constructor(dataset: Record<string, string>) {
    this.dataset = dataset;
  }

  addEventListener(type: string, handler: () => void) {
    const handlers = this.listeners.get(type) ?? new Set<() => void>();
    handlers.add(handler);
    this.listeners.set(type, handlers);
  }

  removeEventListener(type: string, handler: () => void) {
    this.listeners.get(type)?.delete(handler);
  }

  dispatch(type: string) {
    for (const handler of this.listeners.get(type) ?? []) {
      handler();
    }
  }
}

class FakeTooltipElement {
  style = { display: "none" };
  textContent = "";
  currentHtml = "";
  previewRemoved = false;
  insertedContent: FakeNode | null = null;
  existingContent: FakeNode | null = null;
  metaNode: FakeNode = { className: "tooltip-meta", textContent: "" };
  image: FakeImageElement | null = null;
  offsetWidth = 320;
  offsetHeight = 160;

  set innerHTML(value: string) {
    this.currentHtml = value;
    this.previewRemoved = false;
    this.insertedContent = null;
    this.existingContent = null;
    this.image = null;

    const imageMatch = value.match(/<img[^>]+data-fallback-content="([^"]*)"[^>]*>/);
    if (imageMatch) {
      this.image = new FakeImageElement({
        fallbackContent: imageMatch[1]
          .replace(/&quot;/g, "\"")
          .replace(/&#39;/g, "'")
          .replace(/&lt;/g, "<")
          .replace(/&gt;/g, ">")
          .replace(/&amp;/g, "&"),
      });
    }

    const textMatch = value.match(/<div class="tooltip-content">([\s\S]*?)<\/div>/);
    if (textMatch) {
      this.existingContent = {
        className: "tooltip-content",
        textContent: textMatch[1]
          .replace(/&quot;/g, "\"")
          .replace(/&#39;/g, "'")
          .replace(/&lt;/g, "<")
          .replace(/&gt;/g, ">")
          .replace(/&amp;/g, "&"),
      };
    }
  }

  get innerHTML() {
    return this.currentHtml;
  }

  querySelector(selector: string) {
    if (selector === ".tooltip-image-preview") {
      return this.image && !this.previewRemoved
        ? {
          remove: () => {
            this.previewRemoved = true;
          },
        }
        : null;
    }

    if (selector === "img[data-request-id]") {
      return this.image;
    }

    if (selector === ".tooltip-content") {
      return this.insertedContent ?? this.existingContent;
    }

    if (selector === ".tooltip-meta") {
      return this.metaNode;
    }

    return null;
  }

  insertBefore(node: FakeNode, referenceNode: FakeNode | null) {
    assert.equal(referenceNode, this.metaNode);
    this.insertedContent = node;
  }
}

function createTooltipRuntimeHarness() {
  const tooltip = new FakeTooltipElement();
  const timeouts = new Map<number, () => void>();
  let nextTimeoutId = 1;
  const tooltipReadyCalls: Array<{ requestId: number; width: number; height: number }> = [];

  const context = {
    console,
    window: {
      __TAURI_INTERNALS__: {
        invoke: (_command: string, payload: { requestId: number; width: number; height: number }) => {
          tooltipReadyCalls.push(payload);
          return { catch: () => undefined };
        },
      },
    },
    document: {
      documentElement: {
        classList: {
          toggle: () => undefined,
        },
      },
      getElementById: (id: string) => {
        assert.equal(id, "tooltip");
        return tooltip;
      },
      createElement: (_tag: string) => ({
        className: "",
        textContent: "",
      }),
    },
    requestAnimationFrame: (callback: () => void) => {
      callback();
      return 1;
    },
    setTimeout: (callback: () => void) => {
      const id = nextTimeoutId++;
      timeouts.set(id, callback);
      return id;
    },
    clearTimeout: (id: number) => {
      timeouts.delete(id);
    },
  };

  context.window.window = context.window;
  context.window.document = context.document;
  vm.runInNewContext(extractTooltipScript(), context);

  return {
    tooltip,
    tooltipReadyCalls,
    showTooltip: context.window.showTooltip as (requestId: number, html: string, theme: string) => void,
    runTimeout(id: number) {
      const callback = timeouts.get(id);
      assert.ok(callback, `timeout ${id} 应存在`);
      timeouts.delete(id);
      callback();
    },
    getPendingTimeoutIds() {
      return [...timeouts.keys()];
    },
  };
}

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
  assert.doesNotMatch(html, /#tooltip\{max-width:\d+px !important}/);
  assert.match(html, /meta-size">1920 × 1080</);
  assert.match(html, /--tooltip-image-max-width:\s*560px/);
  assert.match(html, /--tooltip-image-max-height:\s*420px/);
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

test("picker 图片预览使用更大的缩略图与 tooltip 上限", () => {
  assert.equal(PICKER_IMAGE_THUMBNAIL_SIZE, 72);
  assert.equal(TOOLTIP_IMAGE_PREVIEW_MAX_WIDTH, 560);
  assert.equal(TOOLTIP_IMAGE_PREVIEW_MAX_HEIGHT, 420);
});

test("图片 tooltip 加载失败后会回退为纯文本并重新测量", () => {
  const html = buildTooltipHtml(
    {
      id: "demo-3",
      type: "image",
      contentPreview: "图片预览摘要",
      tooltipText: "图片加载失败时的回退文本",
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
      imageUrl: "asset://demo-3",
      requestId: 9,
    },
  );
  const harness = createTooltipRuntimeHarness();

  harness.showTooltip(9, html, "dark");
  const image = harness.tooltip.image;
  assert.ok(image, "图片 tooltip 应创建 img 节点");

  image.dispatch("error");

  assert.equal(harness.tooltip.previewRemoved, true);
  assert.equal(harness.tooltip.insertedContent?.className, "tooltip-content");
  assert.equal(harness.tooltip.insertedContent?.textContent, "图片加载失败时的回退文本");
  assert.equal(harness.tooltipReadyCalls.at(-1)?.requestId, 9);
});
