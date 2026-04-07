# Custom Theme Colors Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 FloatPaste 增加浅色 / 深色两套自定义窗口背景色、卡片背景色与强调色，并让 Tooltip 同步跟随。

**Architecture:** 先扩展设置模型并补齐颜色清洗与默认回退，再在前端主题层引入运行时 token 覆盖和派生逻辑，最后把设置页输入、mock backend 与 tooltip 页面接到同一套颜色应用链路上。组件层尽量继续依赖现有 `pg-*` 语义 token，避免把颜色读取逻辑散落到多个界面中。

**Tech Stack:** React 19、TypeScript、Tauri 2、Rust、Node `node:test`

---

## Chunk 1: 设置模型与颜色工具

### Task 1: 为前后端设置模型补齐自定义颜色字段

**Files:**
- Modify: `src/shared/types/settings.ts`
- Modify: `src-tauri/src/domain/settings.rs`
- Modify: `src/bridge/mockBackend.ts`
- Test: `src-tauri/src/domain/settings.rs`

- [ ] **Step 1: 写 Rust 侧失败测试，覆盖新字段默认值与旧配置回退**

```rust
#[test]
fn custom_theme_colors_defaults_are_available() {
    let settings = UserSetting::default();
    assert_eq!(settings.custom_theme_colors.light.accent, "#0969DA");
}

#[test]
fn deserialize_old_settings_without_custom_theme_colors_uses_defaults() {
    let settings: UserSetting = serde_json::from_str(r#"{"shortcut":"Alt+Q"}"#).unwrap();
    assert_eq!(settings.custom_theme_colors.dark.card_bg, "#2E333C");
}
```

- [ ] **Step 2: 运行失败测试确认当前行为未覆盖新字段**

Run: `rtk cargo test custom_theme_colors --manifest-path src-tauri/Cargo.toml`
Expected: FAIL，提示 `UserSetting` 或断言中缺少 `custom_theme_colors`

- [ ] **Step 3: 最小实现 Rust 结构与默认值**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeColorPalette {
    pub window_bg: String,
    pub card_bg: String,
    pub accent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomThemeColors {
    pub light: ThemeColorPalette,
    pub dark: ThemeColorPalette,
}
```

- [ ] **Step 4: 补前端类型与 mock backend 字段**

```ts
export type ThemeColorPalette = {
  windowBg: string;
  cardBg: string;
  accent: string;
};
```

- [ ] **Step 5: 运行测试确认通过**

Run: `rtk cargo test custom_theme_colors --manifest-path src-tauri/Cargo.toml`
Expected: PASS

## Chunk 2: 颜色清洗与主题运行时 token

### Task 2: 提取前端纯函数并先写 node:test

**Files:**
- Create: `src/shared/themeColors.ts`
- Create: `tests/themeColors.test.ts`
- Modify: `src/shared/theme.ts`
- Modify: `src/index.css`

- [ ] **Step 1: 写失败测试，覆盖 hex 校验、默认回退和 tooltip token 导出**

```ts
test("invalid custom colors fall back to defaults", () => {
  const colors = sanitizeCustomThemeColors({
    light: { windowBg: "blue", cardBg: "#fff", accent: "#123456" },
    dark: DEFAULT_CUSTOM_THEME_COLORS.dark,
  });
  assert.equal(colors.light.windowBg, DEFAULT_CUSTOM_THEME_COLORS.light.windowBg);
});

test("buildThemeCssVariables returns accent rgb and subtle values", () => {
  const vars = buildThemeCssVariables("light", DEFAULT_CUSTOM_THEME_COLORS);
  assert.equal(vars["--pg-user-accent-rgb"], "9, 105, 218");
});
```

- [ ] **Step 2: 运行失败测试**

Run: `rtk node --test tests/themeColors.test.ts`
Expected: FAIL，提示模块或函数不存在

- [ ] **Step 3: 实现最小颜色工具**

```ts
export function sanitizeHexColor(value: string, fallback: string): string {
  return /^#[0-9a-fA-F]{6}$/.test(value.trim()) ? value.trim().toUpperCase() : fallback;
}
```

- [ ] **Step 4: 在 `useAppliedTheme` 中接入颜色应用**

```ts
applyThemeColors(root, resolvedTheme, customThemeColors);
```

- [ ] **Step 5: 在 `src/index.css` 中增加运行时变量 fallback**

```css
--pg-user-window-bg: var(--pg-canvas-default);
--pg-user-card-bg: var(--pg-canvas-subtle);
--pg-user-accent: var(--pg-blue-5);
```

- [ ] **Step 6: 重新运行前端颜色测试**

Run: `rtk node --test tests/themeColors.test.ts`
Expected: PASS

## Chunk 3: 设置页与自动保存输入

### Task 3: 为设置页添加浅色 / 深色十六进制输入

**Files:**
- Modify: `src/features/settings/SettingsShell.tsx`
- Modify: `src/bridge/commands.ts`
- Modify: `src/bridge/mockBackend.ts`
- Test: `tests/mockBackendSummary.test.ts`

- [ ] **Step 1: 写失败测试，确认 mock backend 会清洗非法颜色**

```ts
test("mockUpdateSettings sanitizes invalid custom colors", async () => {
  const next = await mockUpdateSettings({ ...baseSettings, customThemeColors: brokenColors });
  assert.equal(next.customThemeColors.light.windowBg, DEFAULT_CUSTOM_THEME_COLORS.light.windowBg);
});
```

- [ ] **Step 2: 运行失败测试**

Run: `rtk node --test tests/mockBackendSummary.test.ts`
Expected: FAIL，提示 `customThemeColors` 或断言不成立

- [ ] **Step 3: 在 `SettingsShell` 新增受控字段与错误提示**

```ts
const [lightWindowBg, setLightWindowBg] = useState(DEFAULT...);
const colorErrors = getThemeColorErrors(editableColors);
```

- [ ] **Step 4: 将颜色字段纳入自动保存 payload**

```ts
customThemeColors: {
  light: { windowBg: lightWindowBg, cardBg: lightCardBg, accent: lightAccent },
  dark: { windowBg: darkWindowBg, cardBg: darkCardBg, accent: darkAccent },
},
```

- [ ] **Step 5: 重新运行 mock backend 测试**

Run: `rtk node --test tests/mockBackendSummary.test.ts`
Expected: PASS

## Chunk 4: Tooltip 同步与公共页面样式

### Task 4: 让 tooltip 页面吃到同一套运行时 token

**Files:**
- Modify: `public/tooltip.html`
- Modify: `src/bridge/commands.ts`
- Modify: `src/features/picker/PickerShell.tsx`
- Modify: `src/features/search/SearchShell.tsx`
- Test: `tests/pickerTooltip.test.ts`

- [ ] **Step 1: 写失败测试，验证 `showTooltip` 能接收并应用颜色变量**

```ts
test("tooltip runtime applies custom color variables", () => {
  const harness = createTooltipRuntimeHarness();
  harness.showTooltip(1, "<div>demo</div>", "dark", {
    "--pg-user-card-bg": "#202020",
  });
  assert.equal(harness.tooltip.appliedThemeVars["--pg-user-card-bg"], "#202020");
});
```

- [ ] **Step 2: 运行失败测试**

Run: `rtk node --test tests/pickerTooltip.test.ts`
Expected: FAIL，提示 tooltip runtime 未处理颜色变量

- [ ] **Step 3: 最小实现 tooltip 颜色注入**

```ts
showTooltip(requestId, x, y, html, theme, themeVars)
```

- [ ] **Step 4: 在 Picker / Search 调用时传入当前主题变量**

```ts
getCurrentThemeCssVariables(document.documentElement)
```

- [ ] **Step 5: 重跑 tooltip 测试**

Run: `rtk node --test tests/pickerTooltip.test.ts`
Expected: PASS

## Chunk 5: 汇总验证

### Task 5: 完整验证与清理

**Files:**
- Verify only

- [ ] **Step 1: 运行前端 node 测试**

Run: `rtk node --test tests/themeColors.test.ts tests/mockBackendSummary.test.ts tests/pickerTooltip.test.ts`
Expected: PASS

- [ ] **Step 2: 运行 Rust 设置测试**

Run: `rtk cargo test settings --manifest-path src-tauri/Cargo.toml`
Expected: PASS

- [ ] **Step 3: 运行前端构建验证**

Run: `./scripts/win-pnpm build`
Expected: 构建通过，无 TypeScript 错误

- [ ] **Step 4: 运行 Rust 全量测试**

Run: `./scripts/win-cargo test`
Expected: 测试通过

- [ ] **Step 5: 手工检查**

Run:
- 浅色主题修改三项颜色
- 深色主题修改三项颜色
- hover image / text tooltip

Expected:
- 当前主题立即生效
- 另一主题数据保留
- tooltip 配色一致
