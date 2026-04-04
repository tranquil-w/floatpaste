import test from "node:test";
import assert from "node:assert/strict";
import { mockListRecentItems } from "../src/bridge/mockBackend.ts";

test("mock summary 会透出图片项的图片元数据", async () => {
  const items = await mockListRecentItems(10);
  const imageItem = items.find((item) => item.id === "demo-3");

  assert.ok(imageItem, "应包含 demo-3 图片样例");
  assert.equal(imageItem.type, "image");
  assert.equal(imageItem.imagePath, "images/demo-3.png");
  assert.equal(imageItem.imageWidth, 1920);
  assert.equal(imageItem.imageHeight, 1080);
  assert.equal(imageItem.imageFormat, "png");
  assert.equal(imageItem.fileSize, 2400000);
});

test("mock summary 对非图片项保持空图片字段", async () => {
  const items = await mockListRecentItems(10);
  const textItem = items.find((item) => item.id === "demo-1");

  assert.ok(textItem, "应包含 demo-1 文本样例");
  assert.equal(textItem.type, "text");
  assert.equal(textItem.imagePath, null);
  assert.equal(textItem.imageWidth, null);
  assert.equal(textItem.imageHeight, null);
  assert.equal(textItem.imageFormat, null);
  assert.equal(textItem.fileSize, null);
});
