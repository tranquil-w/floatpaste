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
4. **系统透明关闭：回退代码就位，未实测**（注册表检测 + `material-active` 门控；遇到问题再补）。
5. **重建时序：通过**。show 路径挂载 → 暖表面 → 移上屏，无黑帧/白帧闪烁。
6. **部分重绘幂等：通过**（滚动、光标闪烁、hover 切换下无透明度拖影/累积变浊）。
7. **性能：无回退**（软件渲染侧帧率与现状无可感差异；backdrop 合成在 DWM/GPU 侧）。

**浅色模式的重要实测结论**：浅色 SystemBackdrop Acrylic 的系统 tint 白雾很重（DWM 在材质层把模糊采样混成亮白），应用侧 alpha 从 0.72 压到 0.40 壁纸影依然不可见——这是系统材质形态而非实现缺陷。浅色定位为「轻雾磨砂 + 与行卡拉开层次」（0.40）；「明显透出壁纸」需 SWCA 未文档化 API 自定义 tint（带拖动掉帧与兼容风险），用户拍板不采用。

## Consequences

- 主题 token 新增材质层，且每套预设 × 明暗都要给「材质底色 + 回退不透明色」两态，主题测试矩阵随之扩展。
- 窗口装饰装配（`win32_ext`）增加 backdrop 装配点，show 路径每次执行；DWM 属性失败必须静默回退，不得阻塞窗口创建。
- 全窗 alpha 化后，窗口首次显示前的过渡帧（表面未暖）从「白底」变「透出桌面/backdrop」，`warm_surface` 的暖场语义需复核。
