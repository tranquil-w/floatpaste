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
- **消费层全局**：`ui/theme.slint` 导出三个全局——`Theme`（颜色语义 token）、`Metrics`（尺寸刻度）、`Motion`（动效时长与曲线）。`animate` 一律引用 `Motion` 档位（`dur-color` 120ms 颜色类反馈 / `dur-enter` 150ms 浮层进场），不散写字面量（三元关闭态的 `0ms` 占位除外）；速贴/搜索窗的呼出、键盘导航选中、粘贴回车是每日百次级高频交互，除既有进场语言外不加重入动画。

色彩数学（OKLCH/OKLab 转换、WCAG 亮度与对比度）为 `theme.rs` 内原生实现，单元测试与运行时共用同一套函数，保证口径一致。

### 语义规则要点

| Token | 规则 |
|-------|------|
| `fg-default` / `fg-muted` | 相对 canvas ≥ 5.5:1（AA 4.5:1 + 余量） |
| `fg-subtle` | 相对 canvas ≥ 4.5:1（占位符等弱文字仍须 AA） |
| `border-default` | 相对 canvas ≥ 3:1（WCAG 非文本线；这是"边框看得见"的保证） |
| `border-window` | 浮动窗口最外层描边专用，canvas 与 border-muted 的弱混合；窗口边界主要靠阴影与圆角，描边刻意弱化以免框感过重 |
| `accent-fg` | canvas 上的强调文字/图标，≥ 4.5:1，不足则亮度校正 |
| `accent-emphasis` | 实底按钮底，对齐 Windows「蓝底白字」惯例：`fg-on-emphasis` 恒白，原色白字不足 4.5:1 时向黑色 OKLab 混合压暗到刚好达标（色相不变的物理变暗路径；OKLCH 只调亮度在蓝紫段会触发色域裁剪造成色相漂移，故不用） |
| `selected-bg` | 选中项中性底（rgb+alpha 分体）：纯正文墨色低 alpha 的半透明叠层，浅 0.12（乘性压暗 ~-12%）/ 深 0.16（加性提亮 ~+30/255）。叠在玻璃/窗底上亮度增量恒定、不随桌面亮度漂移（不透明固定灰在亮桌面玻璃上会与底趋零乃至反转）；对比断言按合成到 canvas 后的实际色计（深色可见度 ≥ 1.2:1，正文墨字 ≥ 4.5:1）。选中感 = 恒定亮度偏移 + 左缘 3px 强调条 + 标题 semibold（前景通道与底色无关，速贴选中同语言），强调色不铺选中大底。消费点为搜索窗列表；速贴选中（层灰卡白结构）不用底色——白卡群中的深色选中卡有「塌下去」的凹陷感，走 2px 墨框 + semibold |
| `accent-subtle` / `*-subtle` | 由对应前景色 rgba 生成，明暗模式各一组 alpha |
| `selection-fg` / `selection-bg` | 文本选区/IME 组合词高亮：深色 = canvas 向强调色混 0.32 的实底配正文墨字；浅色 = 压暗后的 emphasis 配白字 |
| `surface` | 卡片/浮起面实色：层级语言 = 窗底比卡片深一档，卡片靠亮度差浮起；禁止用 `canvas-subtle` 灰做浅色卡面填充（浅色下贴近实心白层显「蒙灰」）。`card-face` 只消费速贴行卡（浅色取 surface 纯白，深色取 canvas_subtle 微亮——surface 亮灰卡发灰被否；卡面须亮于底一档才浮起）；设置卡不走实色卡面，走 `layer-fill` 半透填充（见下）。速贴选中强调 = 2px `fg-default.with-alpha(0.70)` 墨色边框（与文字同源、随明暗反色：浅加深框/深加浅框，加粗配减淡）+ semibold 字重，无底色变化（选中底色、彩色描边均实测否决）；边框几何经 `PanelGeometry.row-frame`（4px 双侧）参与 Rust 裁排 |
| `material-layer` | 材质窗统一玻璃层：可激活窗（搜索/编辑/设置）窗根直铺（rgb 深色=canvas、浅色=surface 纯白，alpha 明暗同 0.70——玻璃感与前景可读的平衡点；透得更多会被壁纸染色整窗泛色，勿放开）。三窗同款单层写法（`material-active ? material-layer : canvas-default`），设置窗与搜索/编辑观感一致，不另铺实色 tint 层；模糊由 DWM SystemBackdrop 提供（Win32 无自定义 tint 参数、SWCA 带 tint 的老路在 Win11 无模糊）。材质规划：「无 / Mica / Acrylic」三档设置项、每窗一致（见 adr-0005）。速贴是无焦点窗，DWM Acrylic 模糊为激活态特权、失活不渲染，不走本层——唤起时抓屏烘焙模糊+tint 底图（tint 同本 token），抓屏失败回 `card-layer` 实底，见 no-focus-picker.md 坑十一。材质未挂载的窗口回退 `canvas-default` 不透明底，半透明色禁止直接用在无材质的透明窗上（会透出桌面） |
| `layer-fill` | 设置层半透填充（WinUI Layer 同构）：白墨低透叠在玻璃面上，亮度增量恒定、材质随玻璃透底；与预设无关，theme.slint 内由 `is-dark` 直接派生的固定白叠层（深 5% / 浅 70%），不经 Rust token。消费点：设置卡填充与目录选中项（同款偏灰层 = 卡片与选中同语言）。半透明卡面禁止垫 `SoftShadow`（同心实心层透卡显成均匀墨膜），卡靠亮度差分层、无描边无投影；相关设置行合并进同一卡，行间 1px `border-subtle` 分隔线；目录项默认透明，选中 = 本填充 + 3px accent 左竖条 + 文字着色，悬停 = 墨色微叠 |
| 阴影 | 阴影色随正文墨色派生，亮暗共享同一组 shadow token。软件渲染器的 `drop-shadow-*` 是空实现（slint 1.18.1 `draw_box_shadow` 内 `// TODO`，声明被静默忽略），一切投影统一走 `theme.slint` 的 `SoftShadow` 组件（同心外扩实心圆角层堆叠出线性衰减羽化）；半透明卡（速贴玻璃行卡）不能用实心层垫底（透射压暗卡面），用裁剪窗+错位垫底方案（见 picker.slint 行卡投影注释） |

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

窗口主题同步：设置变更经 `theme_bridge.rs` 即时重应用到所有已建窗口；跟随系统模式下 `platform::windows::theme_change` 监听 Personalize 注册表变更通知，系统明暗翻转即重算重应用全窗（通知连发按最近应用快照去重，token 更替伴随可见窗材质重挂）。

## 质量门禁

`theme.rs` 内单元测试覆盖：

- 全预设 × 明暗 × 全部强调色（含迁移 hex 样本）组合，逐项断言四组派生不变量：实底强调白字 ≥4.5:1、选中底墨字（合成到 canvas 后实色计）≥4.5:1、深色档选中可见度 ≥1.2:1、文本选区对比；正文/次级文字/强调/状态色/边框的对比度由 `ensure_contrast` 构造路径保证（default 预设另有参考值逐字段锁定）。调色不达标直接 fail。
- 校正幂等与行为、RGB 通道一致性；token 参考值逐字段断言（键集随之恒定）。
- 旧数据迁移与强调色解析回退。

## 新增预设的步骤

1. 在 `crates/floatpaste-core/src/theme.rs` 新增一对 `*_LIGHT` / `*_DARK` 常量（`PaletteScale`），逐字段填官方色值并在注释注明出处；文字/边框给官方原值即可，对比度由 `ensure_contrast` 校正兜底。
2. 在 `palette_scale()` 的分发中加入该预设，并把 id 加入 `THEME_PRESET_IDS`。
3. 在 `crates/floatpaste-native/src/settings.rs` 的 `preset_names` 补充名称与介绍文案。
4. `cargo test` 确认对比度门禁通过；设置页预设卡片自动出现新预设（预览由 `derive_tokens` 实时派生）。

## 已知边界

- 夜间模式/HDR 的全局色温与色域映射无法在应用内修正；本系统通过对比度冗余与低色度深色基底保证衰减后可读。
- 窗口材质（透明/亚克力/Mica）不在主题系统范围内：其对比度取决于桌面壁纸，跨设备不可控。
