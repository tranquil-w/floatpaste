---
status: accepted
---

# 窗口材质：DWM backdrop 优先

阶段 6「材质 + 动效」的选型记录。FloatPaste 全窗为 Slint 软件渲染（renderer-software + backend-winit，softbuffer 呈现），历史上默认「软件渲染 = 与 DWM 材质无缘」；本轮源码级调研证实**透明链路已经打通且大半是默认行为**，Win11 系统材质（Mica/Acrylic）可行，且不改渲染器、不加依赖。本 ADR 记录证据链、实施方案与 spike 验证清单；spike 全部通过后才翻 accepted 动工。

## 调研结论（源码级证据链）

软件渲染窗口具备 per-pixel alpha 的四个环节，全部已在现有依赖里：

1. **DWM alpha 合成已默认开启**：backend-winit 给每个窗口的默认 `WindowAttributes` 是 `with_transparent(true)`（`winitwindowadapter.rs:662`，`create_window_adapter` 统一走它）；winit 0.30 在 Windows 创建窗口时对此执行 `DwmEnableBlurBehindWindow(DWM_BB_ENABLE | DWM_BB_BLURREGION, CreateRectRgn(0,0,-1,-1))`（`winit/src/platform_impl/windows/window.rs:1231`）——空区域 blur behind 正是「DWM 尊重重定向表面 alpha 通道」的开关。窗口因 `SLINT_DESTROY_WINDOW_ON_HIDE=1` 重建时由 winit 自动重挂。
2. **呈现路径保留 alpha**：softbuffer Windows 后端 = `CreateDIBSection` 32bpp + `BitBlt(SRCCOPY)`（`softbuffer-0.4.8/src/backends/win32.rs:189`），逐位复制包括 alpha 字节。softbuffer 自身不处理 alpha（issue #17），依赖后端开启 blur behind——winit 已代做。
3. **软件渲染器是 alpha 感知的**：`SoftBufferPixel` 的 `TargetPixel` 实现走 premultiplied alpha（`From<PremultipliedRgbaColor>` 保留 alpha，仅不透明快路径 `from_rgb` 写 0xff）；整帧渲染以 `Window.background` blend 进清屏基线（`i-slint-renderer-software lib.rs:1295-1307`）——窗口背景透明时未绘制区域 alpha=0，且部分重绘每行先回填背景基线，半透明像素不随重绘累积变浊（幂等）。
4. **Win11 系统材质接口**：`DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`（值 38，build 22621+）：`DWMSBT_MAINWINDOW`=Mica、`DWMSBT_TRANSIENTWINDOW`=Acrylic、`DWMSBT_TABBEDWINDOW`=Mica Alt。GDI 窗口需配合 `DwmExtendFrameIntoClientArea(margins=-1)` 让 backdrop 铺满客户区（Total Commander 等 GDI 应用实证）；现有 `apply_dwm_shadow` 已在调用该 API（当前 margins 仅底部 1px）。已有约束继续成立：不用 `WS_EX_LAYERED`（与 BitBlt 呈现冲突，`win32_ext.rs` 注释既载）。

生态印证：winit 透明窗配方的独立实现（jeweg/win32-window-transparency）、softbuffer #17 的结论（alpha 需后端开 blur behind）、window-vibrancy 的版本分级（≥22523 用 SystemBackdrop，22000-22522 用非文档化 `DWMWA_MICA_EFFECT=1029`，Win10 1809+ 用未文档化 SWCA accent）。

## 实施方案

- **主路线（Win11 22H2+）**：`DWMWA_SYSTEMBACKDROP_TYPE`——全部窗口统一 `DWMSBT_TRANSIENTWINDOW`（Acrylic）；`DwmExtendFrameIntoClientArea` margins 扩为 -1；`DWMWA_USE_IMMERSIVE_DARK_MODE` 随主题明暗联动。装配点复用现有 show 路径样式装配模式（`apply_dwm_shadow` 同位置），窗口重建后随 show 重挂。初版按 Win11 常驻窗指引给搜索/编辑/设置用 Mica，真机实测视觉过弱（Mica 是壁纸低频混色、不模糊），2026-09-25 用户拍板全量改 Acrylic——本应用是浮层工具家族，观感一致性优先于常驻窗指引。
- **UI 侧**：`.slint` 的 `Window.background` 改透明；主题 token 增「材质底色」层（半透明面板色，深色示例 `rgba(30,30,30,0.75)`），根容器与底栏用它；文字、图标、控件保持不透明保证可读性。主题系统需提供「透明不可用时的回退不透明色」同位切换。
- **版本回退链**：build 22000-22522 用 `DWMWA_MICA_EFFECT=1029`（仅 Mica）；Win10 1809+ 评估未文档化 SWCA `ACCENT_ENABLE_BLURBEHIND`，否则回落 winit 已挂的旧式 blur（alpha=0 区域呈雾面）；全部不可用或系统关闭「透明效果」时保持现状不透明背景——回退是**默认态**，材质是增强。

## 否决项

- **WS_EX_LAYERED + UpdateLayeredWindow**：分层窗口不经 BitBlt 呈现，与 softbuffer 冲突；失去 DWM 阴影/圆角；`win32_ext.rs` 已明令禁用。
- **WS_EX_NOREDIRECTIONBITMAP**：无重定向表面后 GDI `BitBlt` 无处呈现，软件渲染直接失效。
- **换 GPU 渲染器（Skia/femtovg）换透明能力**：违背软件渲染立项初衷（零 GPU/驱动依赖、发版包体），且 Skia+D3D12 有透明渲染为黑的已知缺陷（slint#8496）。材质不值得为它翻渲染栈。

## spike 验证清单（2026-09-25 真机结论，Win11 24H2）

1. **BitBlt alpha → Acrylic 透出：通过**（整链唯一未实证环节）。深色速贴壁纸影清晰可见；历经 alpha 0.78→0.66→0.70 三轮调整定位观感甜点。
2. **阴影/圆角共存：通过**。extend frame -1 后系统按窗口轮廓投影，速贴观感正常；`apply_dwm_shadow` 的 1px 底边被覆盖属预期。
3. **无焦点窗材质：通过**（速贴 NOACTIVATE 窗显示/使用全程材质保持，未见系统降为纯色）。
4. **系统透明关闭：通过**（2026-09-25 实测：注册表 `EnableTransparency=0` + WM_SETTINGCHANGE 广播后唤出搜索窗，`apply_window_backdrop` 返回 false、门控回不透明 canvas，显示正常无天窗无黑块——上次挂载残留在 HWND 上的 backdrop 属性被不透明底完全遮住，无视觉异常；恢复开启后材质随下次唤出回归）。速贴与搜索窗同走 show 时重挂路径，实测覆盖。
5. **重建时序：通过**。show 路径挂载 → 暖表面 → 移上屏，无黑帧/白帧闪烁。
6. **部分重绘幂等：通过**（滚动、光标闪烁、hover 切换下无透明度拖影/累积变浊）。
7. **性能：无回退**（软件渲染侧帧率与现状无可感差异；backdrop 合成在 DWM/GPU 侧）。

**浅色模式的重要实测结论**：浅色 SystemBackdrop Acrylic 的系统 tint 白雾很重（DWM 在材质层把模糊采样混成亮白），应用侧 alpha 从 0.72 压到 0.40 壁纸影依然不可见——这是系统材质形态而非实现缺陷。浅色定位为「轻雾磨砂 + 与行卡拉开层次」（0.40）；「明显透出壁纸」需 SWCA 未文档化 API 自定义 tint（带拖动掉帧与兼容风险），用户拍板不采用。

## 实施修正（2026-09-26，速贴不透明 bug 的根因复盘）

上线后用户报告速贴不透明。逐层实验（最小 Win32 窗 + 四色带像素值 + 双位置对照 + SWCA spike）补齐了对合成机制的认知，实施经历两次翻转后定案：

- **合成机制（实测定死）**：挂 backdrop + extend frame 后（winit 对 transparent 窗自动挂 blur behind），softbuffer 呈现的表面像素 alpha 参与合成——底色 alpha 0.78 时材质透出率仅 22%，视觉等同不透明。**「搜索窗透明、速贴降级」的猜测被证伪：两者观感从来相同**，速贴「不透明」的抱怨实为「透出率低到材质不可感」。
- **transparent 直露方案（透出 100%）已实施并被用户否决**：技术上成立（材质真实透出、随背景实时变化），但视觉不可接受——亮背景下深色 UI 元素与亮雾底对比全乱，速贴与搜索窗原有深色面板观感被破坏，用户判「难看至极」。
- **恢复 material-base 后用户仍不认可**（同机同壁纸截图对比：搜索窗深蓝灰磨砂、速贴均匀深灰）。**最终根因实锤：SystemBackdrop Acrylic 在窗口非前台时自动降级为纯色（系统行为，无开关）——速贴（NOACTIVATE，前台必须留给粘贴目标）恒为非前台，恒降级**；搜索窗（有焦点）则真材质。此前「两者观感相同」的判断只在本机 100% 缩放的特定背景组合下偶然成立。
- **最终方案：速贴自绘模糊底**（截屏落位处桌面 + `image::imageops::blur` + material-base 半透明叠色）。曾选 SWCA 无焦点 Acrylic（`ACCENT_ENABLE_ACRYLICBLURBEHIND` 未文档化 API，GetProcAddress 动态加载）并验证「不随焦点降级、内容正常渲染」，但其模糊半径小、无系统噪点，磨砂质感达不到 SystemBackdrop 观感（用户实测）；且最初期望的 `DesktopAcrylicController` 查证为 Windows App SDK 专有（非 inbox 系统 API,整个 SDK winmd 与 System32 均无,引入需 WinAppSDK 运行时）——自绘是唯一质感可控的路。搜索/编辑/设置（有焦点窗）维持 SystemBackdrop 路线不变。
- **速贴面板底沿用 material-base 半透明色（与搜索窗同构）**：曾试过 transparent 直露 DWM tint（tint 即底色，alpha 0.78→0.88 两轮），用户实测整体偏透且噪点观感粗糙（噪点纹理 0.06 平铺方案一并否决），均已回退——SWCA 只提供「无焦点不降级的模糊」，色调统一交给 material-base。
- **已知取舍**：模糊底是唤出时的静态截图——展示期间（秒级）背景变化不实时更新；截屏若包含其他置顶浮层会一并入图（罕见）；实现用 `win32_ext::capture_screen_region`（GDI BitBlt）+ `image::imageops::blur`（sigma 18），零新增依赖。定位与落地拆分：`compute_picker_position` 先算落位（截屏依赖它），窗口移入前桌面才是「背后内容」。
- **已知取舍**：SWCA 路线（保留代码于 git 历史）拖动时 DWM 短暂显示纯色，且 API 未文档化有版本风险——均为放弃原因而非当前风险。
- **验证方法沉淀**：把速贴平移到不同亮度背景对照面板底色变化；红窗对照需注意红窗自身显示链路（消息泵、Z 序、激活）各自独立成坑，像素判读前先确认对照物真的在画面里。

## 材质分层与窗口分类（2026-09-26，对齐 PowerToys 选型）

对比 PowerToys 各窗口实现（常驻设置窗 `MicaBackdrop`、AdvancedPaste 可激活瞬态 `DesktopAcrylicBackdrop`、ColorPicker 浮层自制 AlwaysActive acrylic、QuickAccess/MeasureTool 自定义 acrylic 参数 tint 0.5 + luminosity opacity 0.96、PowerToys Run 无材质纯自绘）后，按窗口生命周期分流：

- **编辑/设置 → Mica（`DWMSBT_MAINWINDOW`）**：推翻本 ADR 2026-09-25「全量 Acrylic」拍板。当时「Mica 观感过弱」的结论是在「整窗一张半透明膜」前提下测的——材质强度承担了全部观感；引入内容层后，Mica（近实底、无噪点）是常驻内容窗的正确形态。搜索维持 Acrylic（有焦点瞬态，同 AdvancedPaste 规格）。
- **内容层 token `material-layer`（深 0.90 / 浅 0.62；rgb 深色=canvas、浅色=surface 纯白）**：前景（文字/卡片）一律坐这层；`material-base` 只承担设置窗底/侧栏透出（深 0.85 / 浅 0.45）。速贴面板底同换 `material-layer`，即「底往实里调」，对齐 PowerToys 自定义 acrylic 的 luminosity 0.96 方向。浅色调参三轮：首版 0.80/0.48 白上叠白无层次、二版 0.92/0.85 收实成不透明板（用户判「没有 PowerToys 的材质配比」）——终版放开浅色透明度：**「白雾重」结论只适用于 Acrylic，常驻窗的浅色 Mica 是透壁纸 tint 的**（PowerToys 浅色即证），对齐 WinUI Layer=50% 白的语言。
- **设置分组卡恢复不透明填充（canvas-subtle）**：第四轮「分组透明化」针对的是半透明容器叠半透明底的显形算术；窗底近实心后前提不再成立，实色填充无叠加问题。分组卡 = canvas-subtle 填充 + border-muted 描边 + 12px 圆角（PowerToys 卡片三件套的 fill + stroke）。
- **停屏窗主题切换残影**：软渲染按 damage 矩形局部呈现（softbuffer `present_with_damage` 逐矩形 BitBlt），「仅颜色变化」的帧依赖局部渲染器脏区跟踪覆盖整窗，该跟踪在停屏/恢复时序下不可靠（浅↔深切换实测：速贴/搜索/编辑窗残留旧主题色块与透明缺口；reapply 后泵帧不足以修复）。终案 = 各 show 路径 `win32_ext::force_full_repaint`：真实往返一次窗口高度迫使 softbuffer 重分配缓冲、渲染器转 NewBuffer 全量渲染，不依赖脏区；回位延迟 50ms 留一帧渲染间隔，±1px 不可感。
- **明暗切换须重挂可见窗的材质**：Mica/Acrylic 的 tint 明暗跟随 `DWMWA_USE_IMMERSIVE_DARK_MODE`，该属性仅在挂载点设置——开着设置窗切明暗时只换 token 不重挂，可见窗残留旧明暗的材质底（浅色下透出深色 Mica，白 0.45 膜叠出 #818285 灰环，像素采样证实）。`apply_side_effects` 对存活的设置/编辑窗按新明暗重挂；停屏窗由各自 show 路径重挂天然覆盖。
- **设置窗内容区为层卡**（WinUI `LayerFillColorDefault` 同构）：近实心底 + 1px 描边 + 圆角，侧栏留在材质底上形成层次。
- PowerToys 对无激活瞬态的解法（`DesktopAcrylicController` + `SystemBackdropConfiguration.IsInputActive` 恒 true）与速贴的 SWCA HOSTBACKDROP 同题同解，印证 WinAppSDK 不可用前提下 SWCA 是 inbox 等价物。`SetWindowCompositionAttribute` 为未文档化导出、SDK user32.lib 无符号，静态 `#[link]` 链接必失败（LNK2019），改 `GetProcAddress` 动态加载。

## Consequences

- 材质实现按窗口生命周期分三条管线（`overlay::MaterialSurface`）：编辑/设置（Content）走 SystemBackdrop Mica，搜索（Transient）走 SystemBackdrop Acrylic，速贴（HostAcrylic）走 SWCA HOSTBACKDROP + Acrylic；每个窗口固定一条管线，show 路径幂等重挂。
- 材质观感旋钮集中在 `theme.rs` 的 `material_layer_alpha`（内容层，深 0.90/浅 0.62）与 `material_base_alpha`（窗底膜，深 0.85/浅 0.45）；调参经验同向（0.70「幽灵窗」↔ 0.82「看不出」）。
- SWCA 为未文档化 API（`GetProcAddress` 动态加载）：未来 Windows 版本若移除符号，`apply_window_host_backdrop_acrylic` 返回 false 走 canvas 回退；届时可评估回退到 SystemBackdrop（接受无焦点降级）。
- 窗口装饰装配（`win32_ext`）增加 backdrop 装配点，show 路径每次执行；DWM 属性失败必须静默回退，不得阻塞窗口创建。
- 全窗 alpha 化后，窗口首次显示前的过渡帧（表面未暖）从「白底」变「透出桌面/backdrop」，`warm_surface` 的暖场语义需复核。
