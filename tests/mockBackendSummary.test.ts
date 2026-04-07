import test from "node:test";
import assert from "node:assert/strict";
import { mockListRecentItems, mockUpdateSettings } from "../src/bridge/mockBackend.ts";
import { DEFAULT_CUSTOM_THEME_COLORS } from "../src/shared/themeColors.ts";

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

test("mockUpdateSettings 会清洗非法自定义颜色并保留合法值", async () => {
  const updated = await mockUpdateSettings({
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
    searchShortcut: "Alt+S",
    searchShortcutEnabled: true,
    customThemeColors: {
      light: {
        windowBg: "blue",
        cardBg: "#EEF2F5",
        accent: "#123456",
      },
      dark: {
        windowBg: "#151515",
        cardBg: "#22",
        accent: "orange",
      },
    },
  });

  assert.equal(updated.customThemeColors.light.windowBg, DEFAULT_CUSTOM_THEME_COLORS.light.windowBg);
  assert.equal(updated.customThemeColors.light.cardBg, "#EEF2F5");
  assert.equal(updated.customThemeColors.light.accent, "#123456");
  assert.equal(updated.customThemeColors.dark.windowBg, "#151515");
  assert.equal(updated.customThemeColors.dark.cardBg, DEFAULT_CUSTOM_THEME_COLORS.dark.cardBg);
  assert.equal(updated.customThemeColors.dark.accent, DEFAULT_CUSTOM_THEME_COLORS.dark.accent);
});
