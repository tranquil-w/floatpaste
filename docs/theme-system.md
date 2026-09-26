# 主题系统设计

本文档描述 floatpaste 的主题系统架构、约束与扩展方式。

> 设计目标：**界面在任何屏幕（含开启夜间模式、色域不准的外接屏）上保持可读且观感一致**。
> 实现全部在 Rust：原始层与派生层在 `crates/floatpaste-core/src/theme.rs`
> （`PaletteScale` / `derive_tokens` / `ensure_contrast` / `mix_colors`，OKLCH 校正为原生实现），
> 消费层为各窗口的 Slint `Theme` 全局（`crates/floatpaste-native/ui/theme.slint`，
> 由 `theme_bridge.rs` 写入）。

## 背景与问题

旧主题系统在不同电脑上观感差异明显，根因有三：

1. **深色基底蓝调过重**：旧深色中性色为 Primer dark 系蓝灰。Windows 夜间模式削减蓝光时，蓝调灰阶整体黄移，人眼在暗部对色偏最敏感，表现为"深色模式偏色严重、文字看不清"。
2. **对比度余量不足**：边框由 0.10–0.18 的低比例混合生成，次级文字对比度踩在 4.5:1 边缘，屏幕衰减后越过可读线。
3. **派生算法不可控**：用户可自定义底色/卡片色/强调色三个 hex，其余 token 在 sRGB 通道上按硬编码比例线性混合。sRGB 混合不是感知均匀的，中间调在跨 gamma/色域的屏幕上漂移明显，且整条链路无对比度校验。

夜间模式的全局色温变换本身无法在应用内抵消；本系统的策略是**提高对比度冗余、降低对蓝调与低对比边框的依赖**，让界面在偏色与衰减后依然可读。

## 架构：三层 token

```text
原始层 (theme.rs: PaletteScale 常量)     官方配色固化值，注释注明出处
        ↓  theme.rs: derive_tokens（OKLCH 校正与派生）
语义层 (语义 token 键集)                 fg / canvas / border / accent / 状态色
        ↓  theme_bridge.rs 写入
消费层 (ui/theme.slint: Theme 全局)      各窗口组件只允许消费语义 token
```

- **原始层**：每个预设在单一明暗模式下提供一份角色化色板（`PaletteScale`：canvas/surface/ink/border/accent/状态色等）。色值为社区配色官方原值，出处写在注释里。
- **派生层**（`theme.rs`）：纯函数，从色板 + 强调色派生全套语义 token。文字/边框/强调文字先取官方原值，对比度不足时经 `ensure_contrast` 在 OKLCH 空间只调亮度校正达标（幂等，已达标不动）；混合一律走 OKLab 插值（`mix_colors`），不用 sRGB 通道线性混合。
- **语义层**：token 键集恒定，运行时由 `theme_bridge.rs` 写入各窗口的 Slint `Theme` 全局；组件**不得**直接引用原始色阶。

色彩数学（OKLCH/OKLab 转换、WCAG 亮度与对比度）为 `theme.rs` 内原生实现，单元测试与运行时共用同一套函数，保证口径一致。

### 语义规则要点

| Token | 规则 |
|-------|------|
| `fg-default` / `fg-muted` | 相对 canvas ≥ 5.5:1（AA 4.5:1 + 余量） |
| `fg-subtle` | 相对 canvas ≥ 4.5:1（占位符等弱文字仍须 AA） |
| `border-default` | 相对 canvas ≥ 3:1（WCAG 非文本线；这是"边框看得见"的保证） |
| `border-window` | 浮动窗口最外层描边专用，canvas 与 border-muted 的弱混合；窗口边界主要靠阴影与圆角，描边刻意弱化以免框感过重 |
| `accent-fg` | canvas 上的强调文字/图标，≥ 4.5:1，不足则亮度校正 |
| `accent-emphasis` | 实底按钮底，保持官方饱和原色；前景黑白自动二选一，均不足 4.5:1 时推移底色亮度 |
| `accent-subtle` / `*-subtle` | 由对应前景色 rgba 生成，明暗模式各一组 alpha |
| `material-base` | 材质窗根底：canvas 直通 rgb + alpha（深 0.85 / 浅 0.45），铺在 DWM 材质（Mica/Acrylic，见 adr-0005）之上，承担窗缘/底层的材质透出；材质未挂载的窗口回退 `canvas-default` 不透明底，半透明色禁止直接用在无材质的透明窗上（会透出桌面） |
| `material-layer` | 材质窗内容层：rgb 深色=canvas、浅色=surface 纯白；alpha 深 0.90（近实心压噪点）/ 浅 0.62（对齐 WinUI Layer=50% 白，让浅色 Mica 的壁纸 tint 透出）。前景文字与卡片一律坐这层；消费点为速贴整面板、搜索整窗、编辑整窗、设置内容层卡。回退规则同 `material-base` |
| 阴影 | 阴影色随正文墨色派生，亮暗共享同一组 shadow token |

## 预设与强调色

- **两维结构**：模式（跟随系统 / 浅色 / 深色）× 风格预设。每个预设必须同时提供亮暗两版，`themeMode` 的"跟随系统"行为不变。
- **预设**：默认（Radix Colors slate 基调）、Catppuccin（Latte/Mocha）、Tokyo Night（day/night）。深色基底统一低色度化。
- **强调色安全列表**：8 档 Radix 色相（蓝/靛/青/绿/橙/红/紫/粉），亮暗各给基准值，派生时再校正。黄色系因无法在浅底达标而刻意不收录。
- **自由底色自定义已移除**：自由底色是跨设备观感漂移的放大器，且无法保证对比度。个性化收敛为"预设 + 强调色"。

## 数据结构与迁移

设置字段（`core::domain::settings::UserSetting`）：

- `themeMode`: `"system" | "light" | "dark"`
- `themePreset`: 预设 id，非法值回退 `"default"`
- `themeAccent`: `"default"`（跟随预设）| 安全列表 id | 旧版迁移保留的 `#RRGGBB`；非法值在派生时回退预设自带强调色

迁移规则（`sanitized()`）：旧 `customThemeColors` 的底色直接丢弃、由预设接管；亮/暗强调色中与旧默认值不同的合法 hex 保留为 `themeAccent`。`custom_theme_colors` 字段仅反序列化旧配置时读取（`skip_serializing`），不再写回磁盘。

窗口主题同步：设置变更经 `theme_bridge.rs` 即时重应用到所有已建窗口；跟随系统模式下监听系统明暗变化并联动。

## 质量门禁

`theme.rs` 内单元测试覆盖：

- 全预设 × 明暗 × 全部强调色（含迁移 hex 样本）组合，逐项断言正文/次级文字/强调/状态色/按钮/边框对比度。调色不达标直接 fail。
- token 键集恒定、校正幂等与行为、RGB 通道一致性。
- 旧数据迁移与强调色解析回退。

## 新增预设的步骤

1. 在 `crates/floatpaste-core/src/theme.rs` 新增一对 `*_LIGHT` / `*_DARK` 常量（`PaletteScale`），逐字段填官方色值并在注释注明出处；文字/边框给官方原值即可，对比度由 `ensure_contrast` 校正兜底。
2. 在 `palette_scale()` 的分发中加入该预设，并把 id 加入 `THEME_PRESET_IDS`。
3. 在 `crates/floatpaste-native/src/settings.rs` 的 `preset_names` 补充名称与介绍文案。
4. `cargo test` 确认对比度门禁通过；设置页预设卡片自动出现新预设（预览由 `derive_tokens` 实时派生）。

## 已知边界

- 夜间模式/HDR 的全局色温与色域映射无法在应用内修正；本系统通过对比度冗余与低色度深色基底保证衰减后可读。
- 窗口材质（透明/亚克力/Mica）不在主题系统范围内：其对比度取决于桌面壁纸，跨设备不可控。
