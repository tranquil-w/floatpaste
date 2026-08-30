import test from "node:test";
import assert from "node:assert/strict";
import { mockListRecentItems, mockUpdateSettings } from "../src/bridge/mockBackend.ts";
import type { UserSetting } from "../src/shared/types/settings.ts";

function baseSettings(): UserSetting {
  return {
    shortcut: "Alt+Q",
    launchOnStartup: false,
    silentOnStartup: false,
    historyLimit: 1000,
    pickerRecordLimit: 50,
    pickerPositionMode: "mouse",
    excludedApps: [],
    restoreClipboardAfterPaste: true,
    pauseMonitoring: false,
    themeMode: "system",
    themePreset: "default",
    themeAccent: "default",
    searchShortcut: "Alt+S",
    searchShortcutEnabled: true,
    pickerDigitShortcutsEnabled: true,
  };
}

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

test("mockUpdateSettings 会清洗非法主题字段并保留合法值", async () => {
  const updated = await mockUpdateSettings({
    ...baseSettings(),
    themePreset: "solarized",
    themeAccent: "#123456",
  });
  assert.equal(updated.themePreset, "default", "未知预设 id 应回退默认");
  assert.equal(updated.themeAccent, "#123456", "合法迁移 hex 应保留");

  const fallback = await mockUpdateSettings({
    ...baseSettings(),
    themePreset: "catppuccin",
    themeAccent: "blue",
  });
  assert.equal(fallback.themePreset, "catppuccin", "合法预设 id 应保留");
  assert.equal(fallback.themeAccent, "blue", "合法安全列表 id 应保留");

  const invalid = await mockUpdateSettings({
    ...baseSettings(),
    themePreset: "catppuccin",
    themeAccent: "chartreuse",
  });
  assert.equal(invalid.themeAccent, "default", "非法强调色应回退跟随预设");
});
