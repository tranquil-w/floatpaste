import test from "node:test";
import assert from "node:assert/strict";
import { MOCK_IMAGE_URL, createImageUrlResolver } from "../src/bridge/imageUrl.ts";

test("Tauri 分支会先解析图片路径再转换为可渲染 URL", async () => {
  const resolver = createImageUrlResolver({
    isTauriRuntime: () => true,
    resolveImagePath: async (imagePath) => `C:\\floatpaste-data\\${imagePath}`,
    convertFileSrc: (absolutePath) => `asset://${absolutePath}`,
  });

  const imageUrl = await resolver("images/demo-3.png");

  assert.equal(imageUrl, "asset://C:\\floatpaste-data\\images/demo-3.png");
});

test("路径解析失败时返回 null", async () => {
  const resolver = createImageUrlResolver({
    isTauriRuntime: () => true,
    resolveImagePath: async () => {
      throw new Error("missing");
    },
    convertFileSrc: (absolutePath) => `asset://${absolutePath}`,
  });

  const imageUrl = await resolver("images/missing.png");

  assert.equal(imageUrl, null);
});

test("浏览器 mock 分支直接返回占位图 URL", async () => {
  const resolver = createImageUrlResolver({
    isTauriRuntime: () => false,
    resolveImagePath: async (imagePath) => imagePath,
    convertFileSrc: (absolutePath) => absolutePath,
  });

  const imageUrl = await resolver("images/demo-3.png");

  assert.equal(imageUrl, MOCK_IMAGE_URL);
});

test("空路径直接返回 null", async () => {
  const resolver = createImageUrlResolver({
    isTauriRuntime: () => true,
    resolveImagePath: async (imagePath) => imagePath,
    convertFileSrc: (absolutePath) => absolutePath,
  });

  assert.equal(await resolver(null), null);
  assert.equal(await resolver(""), null);
});
