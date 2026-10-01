---
status: accepted
note: 速贴（无焦点窗）不走 DWM 材质管线（Acrylic 模糊是激活态特权），模糊底自绘烘焙，见 no-focus-picker.md 坑十一；可激活窗（搜索/编辑/设置）管线见本文
---

# 窗口材质：DWM backdrop 优先

FloatPaste 全窗为 Slint 软件渲染（renderer-software + backend-winit，softbuffer 呈现）。本 ADR 记录「软件渲染窗口如何获得系统材质（Mica/Acrylic）」的调研结论与实施决策；速贴的无焦点窗特例（自绘模糊）见 no-focus-picker.md 坑十一。

## 决策

- **可激活窗（搜索/编辑/设置）统一 SystemBackdrop Acrylic**：`overlay::apply_material` 收口（明暗/回退门控统一），show 路径幂等重挂；窗根单层 `material-layer` 玻璃直铺，各窗观感一致。Mica 仅作备选：近实底、透壁纸 tint（随壁纸染色不可控），只有窗缘感没有玻璃感。
- **规划**：材质做成设置项「无 / Mica / Acrylic」三档、每窗一致。「无」= `material_active=false` 的既有回退路径；Mica/Acrylic = `apply_window_backdrop` 的既有 transient 参数；届时恢复按档分流。
- **速贴不走本管线**：无焦点窗（NOACTIVATE 常态失活）拿不到 Acrylic 模糊，模糊底为唤起时自绘烘焙（`core::backdrop`）。

## 调研结论（源码级证据）

1. **软渲透明链路是默认行为**：软渲支持透明背景（alpha 表面），winit backend 在 Windows 上用 `WS_EX_NOREDIRECTIONBITMAP` + DWM 合成，softbuffer 的 BitBlt 呈现保留 alpha——软件渲染 ≠ 与 DWM 材质无缘，无需改渲染器或加依赖。
2. **Win11 系统材质接口**：`DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`（值 38，build 22621+）：`DWMSBT_MAINWINDOW`=Mica、`DWMSBT_TRANSIENTWINDOW`=Acrylic。GDI 窗口需配合 `DwmExtendFrameIntoClientArea(margins=-1)` 让 backdrop 铺满客户区。
3. **约束**：不用 `WS_EX_LAYERED`（与 BitBlt 呈现冲突）；系统关闭「透明效果」或属性不支持时返回 false 静默回退——回退是默认态，材质是增强。
4. **版本回退链**：build 22000-22522 用 `DWMWA_MICA_EFFECT=1029`（仅 Mica）；更早版本回落不透明背景。

## 关键约束（防坑）

- **Acrylic 模糊是激活态特权**：失活窗口只剩「无模糊的半透明」（条纹振幅实验：激活态 8.4 / 失活态 29.1）。可激活窗正常；无焦点窗必须自绘。判读「材质活不活」须分两维：透底（纯色垫底差分）与模糊（条纹振幅），垫底判据用整块纯色、模糊判据用条纹，别混用（条纹上做差分恒 0）。
- **窗口活性前提**：从未被激活的窗口（含建窗即 NOACTIVATE）挂 backdrop 恒降级平色；激活后挂载，此后转 NOACTIVATE/TOOLWINDOW、剥 SYSMENU、停屏移位均保活。
- **浅色 Acrylic 系统 tint 白雾重**：浅色观感定位为轻雾磨砂，不追求壁纸影；勿用透明底直露（100% 材质——亮背景下深色 UI 对比全乱）。
- **透明度不放开**：`material-layer` alpha 明暗同 0.70（玻璃感与前景可读、壁纸染色的平衡点）；透得更多会被壁纸染色整窗泛色。
- **玻璃上勿叠双层半透膜**：两层 0.70 合成不透明度 ~0.91（0.70+0.70≠0.70，按合成算术算），深色下比单层实一档近黑；玻璃上的浮层卡填充透明，层次由描边 + 投影承担。
- **明暗切换须重挂可见窗的材质**：tint 明暗跟随 `DWMWA_USE_IMMERSIVE_DARK_MODE`，该属性仅在挂载点设置——只换 token 不重挂，可见窗残留旧明暗的材质底。
- **停屏窗主题切换残影**：软渲按 damage 矩形局部呈现，停屏/恢复时序下脏区跟踪不可靠；各 show 路径 `force_full_repaint` 强制全量渲染，不依赖脏区。
- **extend frame 会覆盖 `apply_dwm_shadow` 的 1px 底边**：backdrop 窗口由 DWM 按窗口轮廓投影与圆角，无需保留。

## Consequences

- 材质挂载收口在 `overlay::apply_material`（恒 Acrylic），搜索/编辑/设置共用；观感旋钮集中在 `theme.rs` 的 `material_layer_alpha`。
- 窗口装饰装配（`win32_ext`）在 show 路径执行 backdrop 挂载；DWM 属性失败静默回退，不得阻塞窗口创建。
- 窗口首次显示前的过渡帧（表面未暖）会透出桌面/backdrop，`warm_surface` 暖场语义随透明链路成立。
