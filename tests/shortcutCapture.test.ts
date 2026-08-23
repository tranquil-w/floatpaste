import test from "node:test";
import assert from "node:assert/strict";
import { captureShortcut } from "../src/features/settings/shortcutCapture.ts";

function keyEvent(overrides: Partial<Parameters<typeof captureShortcut>[0]> = {}) {
  return {
    key: "q",
    code: "KeyQ",
    altKey: true,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...overrides,
  };
}

test("组合键按固定顺序生成为可注册的快捷键字符串", () => {
  assert.deepEqual(captureShortcut(keyEvent({ altKey: true })), {
    kind: "complete",
    value: "Alt+Q",
  });

  assert.deepEqual(
    captureShortcut(
      keyEvent({ key: "p", code: "KeyP", ctrlKey: true, altKey: true, shiftKey: true }),
    ),
    { kind: "complete", value: "Ctrl+Alt+Shift+P" },
  );
});

test("数字与 F 键可以作为主键", () => {
  assert.deepEqual(
    captureShortcut(keyEvent({ key: "1", code: "Digit1", altKey: true })),
    { kind: "complete", value: "Alt+1" },
  );
  assert.deepEqual(
    captureShortcut(keyEvent({ key: "F5", code: "F5", altKey: false, ctrlKey: true })),
    { kind: "complete", value: "Ctrl+F5" },
  );
});

test("仅修饰键时保持等待，不产出结果", () => {
  assert.deepEqual(
    captureShortcut(keyEvent({ key: "Alt", code: "AltLeft", altKey: true })),
    { kind: "pending" },
  );
});

test("无修饰键或仅 Shift 修饰时拒绝，避免抢占正常打字", () => {
  assert.deepEqual(captureShortcut(keyEvent({ altKey: false })), {
    kind: "invalid",
    reason: "请至少配合 Ctrl、Alt 或 Win 修饰键，避免影响正常打字",
  });
  assert.deepEqual(
    captureShortcut(keyEvent({ altKey: false, shiftKey: true, key: "Shift", code: "ShiftLeft" })),
    { kind: "pending" },
  );
});

test("Escape 取消录制，Backspace 清空已有快捷键", () => {
  assert.deepEqual(captureShortcut(keyEvent({ key: "Escape", code: "Escape" })), {
    kind: "cancel",
  });
  assert.deepEqual(captureShortcut(keyEvent({ key: "Backspace", code: "Backspace" })), {
    kind: "clear",
  });
});

test("不支持的主键给出可读原因", () => {
  const result = captureShortcut(keyEvent({ key: "[", code: "BracketLeft", ctrlKey: true }));
  assert.equal(result.kind, "invalid");
  if (result.kind === "invalid") {
    assert.match(result.reason, /不支持的按键/);
  }
});
