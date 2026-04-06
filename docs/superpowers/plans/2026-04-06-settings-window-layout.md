# 设置窗口布局优化 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `SettingsShell` 从当前的单列长页表单升级为轻量双栏桌面偏好设置页，同时补齐窗口最小宽度约束、窄窗退化、加载失败阻断态和分组导航联动。

**Architecture:** 保持现有设置数据模型、自动保存逻辑和 Tauri 命令层不变，把改动收敛在三个层面：`manager` 窗口尺寸约束、`SettingsShell` 的页面骨架重构、以及少量薄前端组件/Hook 抽取。导航联动和窄窗退化通过前端受控状态处理，自动保存与错误反馈继续沿用现有 query/mutation 通路。

**Tech Stack:** React 19, TypeScript, Tailwind CSS, TanStack Query, Tauri 2, Rust

**Spec:** `docs/superpowers/specs/2026-04-06-settings-window-layout-design.md`

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Create | `src/features/settings/settingsSections.ts` | 设置分组元数据：id、标题、简述、导航顺序 |
| Create | `src/features/settings/useSettingsNavigation.ts` | 双栏/窄窗布局判定、active section 计算、编程滚动锁定 |
| Create | `src/features/settings/SettingsNav.tsx` | 左侧纵向导航与窄窗顶部锚点的统一渲染 |
| Create | `src/features/settings/SettingsSection.tsx` | 右侧分组壳层：锚点、标题、说明、模块容器 |
| Modify | `src/features/settings/SettingsShell.tsx:1-587` | 加载失败阻断态、双栏骨架、模块化表单、导航联动、窄窗退化 |
| Modify | `src-tauri/tauri.conf.json:14-22` | `manager` 设置窗口默认宽度与最小宽度约束 |
| Modify | `src-tauri/src/services/window_coordinator.rs:35-48,458-473,937-1002` | 设置窗口 builder 尺寸常量与 Rust 配置测试 |

---

## Chunk 1: 窗口约束与导航骨架

### Task 1: 锁定设置窗口尺寸约束并补自动化覆盖

**Files:**
- Modify: `src-tauri/src/services/window_coordinator.rs:35-48,458-473,937-1002`
- Modify: `src-tauri/tauri.conf.json:14-22`

- [ ] **Step 1: 先写会失败的 Rust 配置测试**

在 `src-tauri/src/services/window_coordinator.rs` 的 `#[cfg(test)] mod tests` 里新增一个针对 `manager` 窗口的测试，明确要求：

```rust
#[test]
fn tauri_config_should_pin_settings_window_for_two_column_layout() {
    let config: Value = serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
    let windows = config["app"]["windows"].as_array().unwrap();
    let settings = windows
        .iter()
        .find(|window| window["label"] == "manager")
        .unwrap();

    assert_eq!(settings["width"], Value::from(920));
    assert_eq!(settings["height"], Value::from(760));
    assert_eq!(settings["minWidth"], Value::from(880));
}
```

再新增一个纯常量测试，确保 Rust builder 的尺寸常量不会和 `tauri.conf.json` 脱节：

```rust
#[test]
fn settings_window_constants_should_match_two_column_contract() {
    assert_eq!(SETTINGS_WINDOW_DEFAULT_WIDTH, 920);
    assert_eq!(SETTINGS_WINDOW_DEFAULT_HEIGHT, 760);
    assert_eq!(SETTINGS_WINDOW_MIN_WIDTH, 880);
}
```

- [ ] **Step 2: 运行测试，确认当前配置会失败**

Run: `./scripts/win-cargo test tauri_config_should_pin_settings_window_for_two_column_layout`
Expected: FAIL，因为当前 `manager` 窗口宽度还是 `1200`，且未配置 `minWidth`

- [ ] **Step 3: 在 Rust builder 中抽出设置窗口尺寸常量**

在 `src-tauri/src/services/window_coordinator.rs` 顶部常量区新增：

```rust
pub const SETTINGS_WINDOW_DEFAULT_WIDTH: u32 = 920;
pub const SETTINGS_WINDOW_DEFAULT_HEIGHT: u32 = 760;
pub const SETTINGS_WINDOW_MIN_WIDTH: u32 = 880;
```

并将 `ensure_settings_window()` 里的硬编码：

```rust
.inner_size(1200.0, 760.0)
```

改为：

```rust
.inner_size(
    SETTINGS_WINDOW_DEFAULT_WIDTH as f64,
    SETTINGS_WINDOW_DEFAULT_HEIGHT as f64,
)
.min_inner_size(SETTINGS_WINDOW_MIN_WIDTH as f64, SETTINGS_WINDOW_DEFAULT_HEIGHT as f64)
```

要求：
- 只给 settings 窗口补最小宽度约束，不顺手修改其他窗口
- `resizable(true)`、`center()`、`visible(false)` 保持不变

- [ ] **Step 4: 同步更新 `tauri.conf.json` 的 `manager` 窗口尺寸**

将：

```json
{
  "label": "manager",
  "title": "FloatPaste · 设置",
  "width": 1200,
  "height": 760,
  "resizable": true,
  "center": true,
  "visible": false
}
```

改为：

```json
{
  "label": "manager",
  "title": "FloatPaste · 设置",
  "width": 920,
  "height": 760,
  "minWidth": 880,
  "resizable": true,
  "center": true,
  "visible": false
}
```

- [ ] **Step 5: 重新运行 Rust 测试，确认尺寸契约已经固定**

Run: `./scripts/win-cargo test tauri_config_should_pin_settings_window_for_two_column_layout`
Expected: PASS

Run: `./scripts/win-cargo test settings_window_constants_should_match_two_column_contract`
Expected: PASS

- [ ] **Step 6: 提交窗口契约改动**

```bash
git add src-tauri/tauri.conf.json src-tauri/src/services/window_coordinator.rs
git commit -m "feat(settings): 固定双栏设置窗口尺寸契约"
```

### Task 2: 提取设置分组元数据与导航 Hook 骨架

**Files:**
- Create: `src/features/settings/settingsSections.ts`
- Create: `src/features/settings/useSettingsNavigation.ts`
- Create: `src/features/settings/SettingsNav.tsx`
- Create: `src/features/settings/SettingsSection.tsx`
- Modify: `src/features/settings/SettingsShell.tsx:1-122`

- [ ] **Step 1: 创建分组元数据文件**

新增 `src/features/settings/settingsSections.ts`，集中定义分组 id、标题和短说明：

```ts
export type SettingsSectionId =
  | "shortcuts"
  | "general"
  | "appearance"
  | "behavior"
  | "excludedApps";

export type SettingsSectionMeta = {
  id: SettingsSectionId;
  label: string;
  description: string;
};

export const SETTINGS_SECTIONS: SettingsSectionMeta[] = [
  { id: "shortcuts", label: "快捷键", description: "全局唤起与搜索" },
  { id: "general", label: "通用", description: "历史上限与列表容量" },
  { id: "appearance", label: "外观", description: "主题与窗口位置" },
  { id: "behavior", label: "行为", description: "启动与监听策略" },
  { id: "excludedApps", label: "排除应用", description: "忽略指定进程" },
];
```

要求：
- 所有导航文案统一从这里读取，不再在 `SettingsShell` 里散落硬编码标题

- [ ] **Step 2: 创建导航联动 Hook**

新增 `src/features/settings/useSettingsNavigation.ts`，返回至少这些受控接口：

```ts
type UseSettingsNavigationResult = {
  layoutMode: "sidebar" | "compact";
  activeSectionId: SettingsSectionId;
  registerSection: (id: SettingsSectionId, element: HTMLElement | null) => void;
  scrollToSection: (id: SettingsSectionId) => void;
};
```

实现要求：
- 使用 `ResizeObserver` 或等价方案观察主体可用宽度
- `>= 880px` 返回 `"sidebar"`，否则返回 `"compact"`
- active 判定、`96px` 补偿和 `120ms` 编程滚动锁定按 spec 实现
- Hook 只处理导航状态，不管理设置表单数据

- [ ] **Step 3: 创建薄组件壳层**

新增：

1. `src/features/settings/SettingsNav.tsx`
   - 接收 `items`、`activeSectionId`、`layoutMode`、`onSelect`
   - 在 `"sidebar"` 模式渲染纵向 `nav`
   - 在 `"compact"` 模式渲染横向顶部锚点

2. `src/features/settings/SettingsSection.tsx`
   - 接收 `id`、`title`、`description`、`registerSection`
   - 负责 section 锚点、标题和分组说明的统一壳层

要求：
- 导航项使用原生 `button`
- active 项带 `aria-current="true"`
- `nav` 容器带 `aria-label="设置分组"`

- [ ] **Step 4: 将 `SettingsShell` 的常量与标题文案接到新元数据/组件上**

先只改导入与骨架，不重写大段 JSX：
- 从 `settingsSections.ts` 导入分组元数据
- 从新 Hook 获取 `layoutMode` / `activeSectionId` / `scrollToSection`
- 保留现有表单渲染，先让类型和依赖接通

- [ ] **Step 5: 运行前端构建，确认导航骨架文件通过类型检查**

Run: `./scripts/win-pnpm build`
Expected: PASS，TypeScript 与 Vite 均通过

- [ ] **Step 6: 提交导航骨架改动**

```bash
git add src/features/settings/settingsSections.ts src/features/settings/useSettingsNavigation.ts src/features/settings/SettingsNav.tsx src/features/settings/SettingsSection.tsx src/features/settings/SettingsShell.tsx
git commit -m "feat(settings): 提取设置导航与分组骨架"
```

---

## Chunk 2: 设置页重组与状态收口

### Task 3: 先补齐加载失败阻断态与页头状态层级

**Files:**
- Modify: `src/features/settings/SettingsShell.tsx:124-362`

- [ ] **Step 1: 重写顶部状态区，区分加载中 / 加载失败 / 保存失败**

在 `SettingsShell` 中保留现有自动保存状态源，但把渲染层级改成：
- 页头始终可见
- `settings.isLoading && !data` 时显示 loading 壳，不渲染可编辑表单
- `settings.isError && !data` 时显示阻断式错误卡片 + 重试按钮
- `saveError` 继续作为页头下方的非阻断错误提示

阻断错误态建议结构：

```tsx
<div className="rounded-xl border border-pg-danger-fg/40 bg-pg-danger-subtle px-5 py-4">
  <h2 className="text-sm font-semibold text-pg-danger-fg">设置加载失败</h2>
  <p className="mt-1 text-sm text-pg-fg-muted">未能读取当前配置，请重试。</p>
  <button onClick={() => settings.refetch()} type="button">重新加载</button>
</div>
```

要求：
- 加载失败时不要用本地默认值渲染表单
- 重试入口使用 `settings.refetch()`
- `settings.isLoading && !data` 与 `settings.isError && !data` 时都不要渲染可交互的左侧导航或顶部锚点
- 只有成功拿到 `data` 后，才恢复导航与内容联动 UI

- [ ] **Step 2: 运行构建，确认阻断态不会破坏现有 query 类型**

Run: `./scripts/win-pnpm build`
Expected: PASS

- [ ] **Step 3: 提交错误态收口**

```bash
git add src/features/settings/SettingsShell.tsx
git commit -m "feat(settings): 补齐设置加载失败阻断态"
```

### Task 4: 把 `SettingsShell` 重组为双栏骨架与窄窗退化

**Files:**
- Modify: `src/features/settings/SettingsShell.tsx:332-587`
- Modify: `src/features/settings/SettingsNav.tsx`
- Modify: `src/features/settings/SettingsSection.tsx`

- [ ] **Step 1: 先搭主体骨架，不急着改每个控件**

将当前：

```tsx
<div className="mx-auto w-full max-w-[680px] px-6 py-8">
```

改为双栏主体，例如：

```tsx
<div className="mx-auto w-full max-w-[920px] px-6 py-8">
  <div className="grid gap-8 lg:grid-cols-[220px_minmax(0,1fr)]">
    <SettingsNav ... />
    <div className="min-w-0">...</div>
  </div>
</div>
```

要求：
- `"sidebar"` 模式渲染左侧导航
- `"compact"` 模式在右侧内容前渲染顶部锚点
- 不制造左右双滚动容器

- [ ] **Step 2: 用 `SettingsSection` 包裹五个一级分组**

将当前五个 `<section>` 替换为：

```tsx
<SettingsSection
  id="shortcuts"
  title="快捷键"
  description="全局唤起与搜索相关设置。"
  registerSection={registerSection}
>
  ...
</SettingsSection>
```

要求：
- 五个分组 id 必须与 `SETTINGS_SECTIONS` 一致
- 标题和说明在右侧形成稳定层级
- 分组顺序不变

- [ ] **Step 3: 重组复合模块**

把两个最关键的复合设置做成明确模块：

1. `搜索窗口`
   - 启用开关与快捷键输入放进同一卡片
   - 关闭时输入框禁用但保留值

2. `开机自启`
   - `开机时静默启动` 作为次级项放在同一卡片中
   - 关闭父项时子项禁用且沿用现有数据逻辑清零

要求：
- 只改变布局与主次关系，不重写状态来源
- `SettingsShell` 继续持有这些状态

- [ ] **Step 4: 让导航联动真正可用**

接通：
- 点击导航或顶部锚点后滚动到目标分组
- 滚动内容区后 active 分组同步更新
- 宽度跨越 `880px` 阈值时保留当前 active section，不回顶

- [ ] **Step 5: 运行构建，确认双栏/退化骨架可编译**

Run: `./scripts/win-pnpm build`
Expected: PASS

- [ ] **Step 6: 提交双栏骨架改动**

```bash
git add src/features/settings/SettingsShell.tsx src/features/settings/SettingsNav.tsx src/features/settings/SettingsSection.tsx
git commit -m "feat(settings): 重构设置页为轻量双栏布局"
```

### Task 5: 做一轮桌面验证并收口细节

**Files:**
- Modify: `src/features/settings/SettingsShell.tsx`
- Modify: `src/features/settings/SettingsNav.tsx`
- Modify: `src/features/settings/useSettingsNavigation.ts`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/services/window_coordinator.rs`

- [ ] **Step 1: 运行前端构建作为提交前基线**

Run: `./scripts/win-pnpm build`
Expected: PASS

- [ ] **Step 2: 运行 Rust 测试，确认窗口契约没有回退**

Run: `./scripts/win-cargo test`
Expected: PASS，新增 settings 窗口尺寸测试通过

- [ ] **Step 3: 启动桌面应用做真实交互验证**

Run: `./scripts/win-pnpm tauri dev`
Expected: 设置窗口可正常打开，Esc 可关闭，自动保存仍正常工作

手动验证清单：
1. 打开设置窗口，第一眼先看到结构骨架，而不是一串控件
2. 左侧导航点击任一分组，右侧滚动到正确位置
3. 手动滚动内容时，active 导航不会来回乱跳
4. 窄窗场景下（可通过浏览器预览或手动缩小）会退化为顶部锚点而不是横向挤压
5. `搜索窗口` 模块中关闭启用开关后，输入框禁用但值保留
6. `开机自启` 关闭后，`开机时静默启动` 仍可见，但以次级禁用态展示
7. 初次加载失败时能看到阻断卡片和重试按钮；保存失败时仍保留非阻断错误提示

针对 `useSettingsNavigation` 的定点验证，额外执行一组可重复步骤：
1. 在浏览器预览中将窗口宽度调到 `900px`，确认显示左侧纵向导航
2. 将窗口宽度调到 `879px`，确认切换为顶部横向锚点，且当前 active section 不回到第一页顶部
3. 滚动内容，使“通用”或“外观”的分组标题接近内容区顶部下方约 `96px` 的位置，确认 active 在标题越过该线时切换
4. 点击“排除应用”导航后，在平滑滚动过程中立刻继续滚轮滚动，确认 active 不会短暂跳回中间分组，并在滚动稳定约 `120ms` 后恢复自然跟随
5. 人为制造 `getSettings` 初次失败场景时，确认阻断态下看不到可点击导航；重试成功后导航才恢复

- [ ] **Step 4: 只修验证中发现的布局/联动尾差**

允许修的范围：
- 间距、sticky 定位、锚点滚动补偿、active 切换抖动、错误态层级

不允许顺手扩 scope：
- 不新增搜索设置项
- 不引入新状态字段
- 不把 compact 模式改成下拉菜单或抽屉

- [ ] **Step 5: 再跑一遍构建确认收口完成**

Run: `./scripts/win-pnpm build`
Expected: PASS

- [ ] **Step 6: 提交最终实现**

```bash
git add src/features/settings/SettingsShell.tsx src/features/settings/settingsSections.ts src/features/settings/useSettingsNavigation.ts src/features/settings/SettingsNav.tsx src/features/settings/SettingsSection.tsx src-tauri/tauri.conf.json src-tauri/src/services/window_coordinator.rs
git commit -m "feat(settings): 优化设置窗口双栏布局与导航结构"
```
