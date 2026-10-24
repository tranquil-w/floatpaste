# 主题系统设计

本文档描述 floatpaste 的主题系统架构、约束与扩展方式。

> 设计目标：**界面在任何屏幕（含开启夜间模式、色域不准的外接屏）上保持可读且观感一致**。
> 实现全部在 Rust：原始层与派生层在 `crates/floatpaste-core/src/theme.rs`
> （`PaletteScale` / `derive_tokens` / `ensure_contrast` / `mix_colors`，OKLCH 校正为原生实现），
> 消费层为各窗口的 Slint `Theme` 全局（`crates/floatpaste-native/ui/theme.slint`，
> 由 `theme_bridge.rs` 写入）。

## 设计约束

- 深色基底控制蓝调与色度，降低夜间模式和偏色屏幕对暗部辨识的影响。
- 文字与边框保持对比度余量，派生结果由自动检查兜底。
- 使用预设与安全强调色，混合和校正在感知色彩空间完成；自由底色会扩大跨设备漂移并破坏对比度约束。
- 全局色温和色域映射仍由系统与显示设备决定；主题通过对比度冗余提高可读性。

## 架构：三层 token

```text
原始层 (theme.rs: PaletteScale 常量)     官方配色固化值，注释注明出处
        ↓  theme.rs: derive_tokens（OKLCH 校正与派生）
语义层 (语义 token 键集)                 fg / canvas / border / accent / 状态色
        ↓  theme_bridge.rs 写入
消费层 (ui/theme.slint: Theme 全局)      各窗口组件只允许消费语义 token
```

- **原始层**：每个预设在单一明暗模式下提供一份角色化色板（`PaletteScale`：canvas/surface/ink/border/accent/状态色等；收藏使用独立金色语义种子）。色值为社区配色官方原值，出处写在注释里。
- **派生层**（`theme.rs`）：纯函数，从色板 + 强调色派生全套语义 token。文字/边框/强调文字先取官方原值，对比度不足时经 `ensure_contrast` 在 OKLCH 空间只调亮度校正达标（幂等，已达标不动）；混合一律走 OKLab 插值（`mix_colors`），不用 sRGB 通道线性混合。
- **语义层**：token 键集恒定，运行时由 `theme_bridge.rs` 写入各窗口的 Slint `Theme` 全局；组件**不得**直接引用原始色阶。
- **消费层全局**：`ui/theme.slint` 导出三个全局——`Theme`（颜色语义 token）、`Metrics`（尺寸刻度）、`Motion`（动效时长与曲线）。`animate` 一律引用 `Motion` 档位（`dur-color` 120ms 颜色类反馈 / `dur-enter` 150ms 浮层进场），不散写字面量（三元关闭态的 `0ms` 占位除外）；速贴呼出直接显示最终内容，不施加位移动画；搜索窗保留既有进场语言。键盘导航选中、粘贴回车等高频交互不加重入动画。

色彩数学（OKLCH/OKLab 转换、WCAG 亮度与对比度）为 `theme.rs` 内原生实现，单元测试与运行时共用同一套函数，保证口径一致。

搜索与速贴的上屏动作列表共用 `PasteActionMenu`：浅色使用 `surface` 浮起面、深色使用 `card-face`，以 `border-muted` 与面外柔影分层；高亮使用中性悬停底和窄强调标记，正文保持 `fg-default`。右侧快捷键键帽在操作行内垂直居中，带「默认次级动作」说明时将两行作为整体居中。搜索展开钮使用轻描边与透明常态底，悬停、按压和展开态使用中性叠层，避免在 Acrylic 卡面上嵌入固定深浅底块。动作列表节点常驻，以 `Motion.dur-enter`（150ms）淡入并从下方 6px 归位；关闭立即撤销命中，方向键切换高亮不重播进场，不移动窗口。

搜索行内的粘贴拆分按钮以 `hover-bg` 叠在当前行底上，边框使用 `border-muted`；内部按钮悬停时再叠一层中性反馈。不要使用实色 `card-face` 覆盖选中行，以免深色 Acrylic 下形成突兀的近黑色块。

速贴行内只保留「⋯」更多按钮，常态描边与底色透明，悬停或菜单打开时显示轻描边与中性叠层。菜单用分隔线区分上屏方式和管理操作；删除项消费 `danger-fg`，高亮时使用 `danger-subtle`，待确认显示「确认删除」，不使用容易被误认作成功反馈的对勾。布局及提示规则见 [速贴更多操作](no-focus-picker.md#速贴更多操作)。

搜索结果的摘要行常驻预留操作区（包含删除待确认宽度，单一上屏方式不预留展开钮），操作条与摘要对齐；下方单行元信息和标签使用完整内容列宽，三者均不随悬停、选中或菜单展开改变可用宽度。链接、路径、代码及富文本在图标槽显示用途标识。元信息先列尺寸、数量、格式等内容特征，再列时间与来源，单行省略，收藏星标独占末端位置，长标签限宽。图片缩略图等比容纳，避免裁掉内容边缘。代码预览消费 `Metrics.font-code`，速贴的裁排标尺与显示使用同一字体，保留原有缩进；其他文本继续使用系统字体。上屏动作菜单常显「仅本次」说明，次级快捷键下标注「默认次级动作」，临时选择不改配置。

### 语义规则要点

| Token | 规则 |
|-------|------|
| `fg-default` / `fg-muted` | 相对 canvas ≥ 5.5:1（AA 4.5:1 + 余量） |
| `fg-subtle` | 相对 canvas ≥ 4.5:1（占位符等弱文字仍须 AA） |
| `fg-metadata` | 速贴、搜索窗口和悬浮气泡的来源、时间及列表溢出计数专用；相对实卡、徽章、实际悬停/选中底和 Acrylic 卡面合成底 ≥4.5:1。Acrylic 参考取材覆盖黑、偏亮青色及白色边界；普通说明与占位符继续使用 `fg-muted` / `fg-subtle`，不要混用元信息色 |
| `border-default` | 相对 canvas ≥ 3:1（WCAG 非文本线；这是"边框看得见"的保证） |
| `border-window` | 可激活窗（搜索/编辑/设置）最外层描边专用，canvas 与 border-muted 的弱混合；窗口边界主要靠阴影与圆角，描边刻意弱化以免框感过重。速贴窗口描边不用本 token：无 DWM 投影托底，边界感更依赖描边，用实一档的 `border-muted`（见 `material-layer` 行速贴段） |
| `accent-fg` | canvas 上的强调文字/图标，≥ 4.5:1，不足则亮度校正 |
| `accent-emphasis` | 实底按钮底，对齐 Windows「蓝底白字」惯例：`fg-on-emphasis` 恒白，原色白字不足 4.5:1 时向黑色 OKLab 混合压暗到刚好达标（色相不变的物理变暗路径；OKLCH 只调亮度在蓝紫段会触发色域裁剪造成色相漂移，故不用） |
| `selected-bg` | 选中项中性底（rgb+alpha 分体）：纯正文墨色低 alpha 的半透明叠层，浅 0.12（乘性压暗 ~-12%）/ 深 0.16（加性提亮 ~+30/255）。叠在玻璃/窗底上亮度增量恒定、不随桌面亮度漂移（不透明固定灰在亮桌面玻璃上会与底趋零乃至反转）；对比断言按合成到 canvas 后的实际色计（深色可见度 ≥ 1.2:1，正文墨字 ≥ 4.5:1）。选中感 = 恒定亮度偏移 + 左缘 3px 强调条 + 标题 semibold（前景通道与底色无关，速贴选中同语言），强调色不铺选中大底。消费点为搜索窗列表；速贴选中（层灰卡白结构）不用底色——白卡群中的深色选中卡有「塌下去」的凹陷感，走 2px 墨框 + semibold |
| `accent-hover` | 实底主按钮悬停色：由 `accent-emphasis` 向黑色混合 0.12，明暗同规则；白字对比度须不低于常态，且 ≥4.5:1 |
| `accent-subtle` / 状态色 | 强调色浅底由 `accent-fg` rgba 派生。成功、错误、警告、完成提示先固定浅底 rgb 与 alpha，再相对浅底叠加到 canvas / 卡面 / 内嵌底后的实际颜色校正前景 ≥4.5:1；禁止用校正后的文字色反复改填色 |
| `hover-bg` | 全窗中性悬停共用正文墨色叠层：浅色 4% 压暗、深色 6% 提亮；保持透底和反馈方向。主按钮使用 `accent-hover`，危险操作使用状态色，选中态保持各自语义 |
| `favorite` | 收藏独立使用金色种子（浅 `#967100` / 深 `#D5B642`），相对元信息同组表面校正图标对比度 ≥3:1；与警告色分别派生，不复用警告色 |
| `selection-fg` / `selection-bg` | 文本选区/IME 组合词高亮：深色 = canvas 向强调色混 0.32 的实底配正文墨字；浅色 = 压暗后的 emphasis 配白字 |
| `surface` | 卡片/浮起面实色：层级语言 = 窗底比卡片深一档，卡片靠亮度差浮起；禁止用 `canvas-subtle` 灰做浅色卡面填充（浅色下贴近实心白层显「蒙灰」）。`card-face` 只消费速贴行卡：浅色取 surface 纯白；深色优先取亮于 canvas 的 canvas-subtle，否则由 canvas 向 surface 混合 0.40，保持各预设卡面略亮于承托层（实底对比 ≥1.05:1）；设置卡不走实色卡面，走 `layer-fill` 半透填充（见下）。速贴「无」档浅色面板取 `canvas-default` 近白、行卡取 `surface` 白，以轻微底差 + 1px `border-muted` 完整描边 + 四周柔影共同分层；不要只依赖近白色差或卡片下沿暗线，深色面板取 `card-layer`。Mica 实卡同样使用 `border-muted` 描边与柔影。速贴选中强调 = 2px `fg-default.with-alpha(0.70)` 墨色边框（与文字同源、随明暗反色：浅加深框/深加浅框，加粗配减淡）+ semibold 字重，无底色变化（选中底色、彩色描边均实测否决）；边框几何经 `PanelGeometry.row-frame`（4px 双侧）参与 Rust 裁排 |
| `material-layer` | 材质窗统一玻璃层：可激活窗（搜索/编辑/设置）窗根直铺（rgb 深色=canvas、浅色=surface 纯白，alpha 明暗同 0.70——玻璃感与前景可读的平衡点；透得更多会被壁纸染色整窗泛色，勿放开）。三窗同款单层写法（`material-active ? material-layer : canvas-default`），设置窗与搜索/编辑观感一致，不另铺实色 tint 层；模糊由 DWM SystemBackdrop 提供（Win32 无自定义 tint 参数、SWCA 带 tint 的老路在 Win11 无模糊）。材质档位由设置项「背景材质」（无/Mica/Acrylic）控制；可激活窗的「无」档用实底，Mica/Acrylic 分别挂 DWM 系统材质；挂载失败或系统透明效果关闭时回退实底。路线依据见 [ADR-0005](adr-0005-window-material.md)。速贴是无焦点窗，DWM 材质对它不可用（Acrylic 模糊为激活态特权、失活不渲染；Mica backdrop 对从未激活窗不渲染，实测），不走本层 alpha——唤起时抓屏烘焙模糊+tint 底图，以 tint 分档：Acrylic 档 `backdrop::ACRYLIC_TINT_ALPHA` 0.80，深色经 `backdrop::tint_acrylic` 按模糊背景的加权通道亮度控制提亮：亮度 ≤30 时混入 5% 白色、30–130 之间线性渐退、≥130 不加白，保持暗背景亮度并避免亮背景泛白；浅色将本 token 的 rgb 混入 10% 黑色作为承托 tint，保留 20% 背景取材；白色背景下承托层约 `#EBEBEB`，70% 白色卡面的软件渲染结果约 `#F8F8F8`，卡面与承托层对比须 ≥1.10:1，避免白叠白使轮廓消失；行卡半透玻璃卡；Mica 档 `backdrop::MICA_TINT_ALPHA` 0.92 近不透明（Mica 材质本身不透明）+ 行卡不透明实卡细描边，见 no-focus-picker.md 坑十一。烘焙底图铺满窗卡会盖住 card 自身边框（Slint 子元素画在父边框之上），card 内补环又会被圆角 clip 沿 border 内缘裁掉直边，故窗口描边由 card 之上的独立 1px 描边环承载（`picker.slint` 的 `frame`），三档统一、颜色 `border-muted`（速贴无 DWM 投影托底，边界感比可激活窗的 `border-window` 弱描边更依赖描边本身）。材质未挂载的窗口回退 `canvas-default` 不透明底，半透明色禁止直接用在无材质的透明窗上（会透出桌面） |
| `layer-fill` | 设置层半透填充（WinUI Layer 同构）：白墨低透叠在玻璃面上，亮度增量恒定、材质随玻璃透底；与预设无关，theme.slint 内由 `is-dark` 直接派生的固定白叠层（深 5% / 浅 70%），不经 Rust token。消费点：设置卡填充与目录选中项（同款偏灰层 = 卡片与选中同语言）。半透明卡面禁止垫 `SoftShadow`（同心实心层透卡显成均匀墨膜），卡靠亮度差分层、无描边无投影；相关设置行合并进同一卡，行间 1px `border-subtle` 分隔线；目录项默认透明，选中 = 本填充 + 3px accent 左竖条 + 文字着色，悬停 = 墨色微叠 |
| 阴影 | 浅色阴影取正文墨色，深色阴影取黑色，统一消费 shadow token。软件渲染器的 `drop-shadow-*` 是空实现（slint 1.18.1 `draw_box_shadow` 内 `// TODO`，声明被静默忽略），组件内投影统一走 `theme.slint` 的 `SoftShadow` 组件（同心外扩实心圆角层堆叠出线性衰减羽化）；速贴行卡使用六层柔影（层距 0.75px、下移 1px、峰值 alpha 0.05），实卡影形左右各内收 1.5px、上缘下移 1.5px 且同步减高，减轻左右与上方阴影并保持底部位置；玻璃行卡只保留下方 6px 裁剪窗内的衰减尾部，卡面下的实心影层必须裁掉以防透射压暗（见 picker.slint）。**面板级投影分流**：速贴以 `use_client_side_shadow` 取消窗框扩展和系统圆角请求、保留非客户区渲染；卡片圆角和短影在四周 4px 留边内自绘（六层、层距 0.5px、下移 1px、峰值 alpha 0.08，几何口径见 no-focus-picker.md 坑十三）；tooltip/托盘菜单投影窗口内自绘（四周留边 + `SoftShadow`，`shadow-margin` 12px），装配保留 winit 的透明合成与默认非客户区渲染策略，不主动扩展 DWM 窗框；禁止以禁用非客户区渲染代替关闭阴影（坑十二） |

## 预设与强调色

- **两维结构**：模式（跟随系统 / 浅色 / 深色）× 风格预设。每个预设必须同时提供亮暗两版，`themeMode` 的"跟随系统"行为不变。
- **预设**：默认（Radix Colors slate 基调）、Catppuccin（Latte/Mocha）、Tokyo Night（day/night）。深色基底统一低色度化。
- **强调色安全列表**：8 档 Radix 色相（蓝/靛/青/绿/橙/红/紫/粉），亮暗各给基准值，派生时再校正。黄色系因无法在浅底达标而刻意不收录。
- **个性化入口**：仅提供预设与强调色，避免自由底色破坏对比度约束。

## 数据结构与迁移

设置字段（`core::domain::settings::UserSetting`）：

- `themeMode`: `"system" | "light" | "dark"`
- `themePreset`: 预设 id，非法值回退 `"default"`
- `themeAccent`: `"default"`（跟随预设）| 安全列表 id | 旧版迁移保留的 `#RRGGBB`；非法值在派生时回退预设自带强调色

迁移规则（`sanitized()`）：旧 `customThemeColors` 的底色直接丢弃、由预设接管；亮/暗强调色中与旧默认值不同的合法 hex 保留为 `themeAccent`。`custom_theme_colors` 字段仅反序列化旧配置时读取（`skip_serializing`），不再写回磁盘。

窗口主题同步：设置变更经 `theme_bridge.rs` 即时重应用到所有已建窗口；跟随系统模式下 `platform::windows::theme_change` 监听 Personalize 注册表变更通知，系统明暗翻转即重算重应用全窗（通知连发按最近应用快照去重，token 更替伴随可见窗材质重挂）。

## 质量门禁

`theme.rs` 内单元测试覆盖：

- 全预设 × 明暗 × 强调色（8 档安全色、跟随预设及迁移 hex 样本），逐项断言主按钮常态/悬停白字、选中底正文、文本选区、元信息、状态文字与收藏图标的对比度；元信息与收藏同时覆盖黑、近黑、偏亮青色、白色取材下的 Acrylic 卡面及中性悬停合成色。
- 深色实卡须亮于窗底且对比 ≥1.05:1；浅色 Acrylic 卡面须亮于承托层且在上述参考取材下对比 ≥1.10:1（视觉分层门槛，不代替文字与边框的无障碍对比度要求）；中性悬停须浅色压暗、深色提亮；深色选中底可见度 ≥1.2:1；收藏与警告不得使用相同派生色。调色须通过这些门禁。
- 校正幂等与行为、RGB 通道一致性；材质 token 的参考值断言。
- 旧数据迁移与强调色解析回退。

## 新增预设的步骤

1. 在 `crates/floatpaste-core/src/theme.rs` 新增一对 `*_LIGHT` / `*_DARK` 常量（`PaletteScale`），逐字段填官方色值并在注释注明出处；文字/边框给官方原值即可，对比度由 `ensure_contrast` 校正兜底。
2. 在 `palette_scale()` 的分发中加入该预设，并把 id 加入 `THEME_PRESET_IDS`。
3. 在 `crates/floatpaste-native/src/settings.rs` 的 `preset_names` 补充名称与介绍文案。
4. `cargo test` 确认对比度门禁通过；设置页预设卡片自动出现新预设（预览由 `derive_tokens` 实时派生）。

## 已知边界

- 夜间模式/HDR 的全局色温与色域映射无法在应用内修正；本系统通过对比度冗余与低色度深色基底保证衰减后可读。
- 材质的壁纸取色和动态背景无法由主题完全控制；参考取材与合成对比度检查覆盖约定样本，视觉兼容性仍需按改动范围验证。系统挂载与烘焙链路见 [窗口时序与防坑](no-focus-picker.md)。
