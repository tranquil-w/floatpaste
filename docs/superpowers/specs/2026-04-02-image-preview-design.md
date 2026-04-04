# 剪贴项图片预览

**日期**: 2026-04-02
**状态**: 已修订

## 概述

为 Picker 面板中的图片类型剪贴项补齐预览体验：

- 列表项显示 32 × 32 缩略图
- 鼠标悬浮时通过原生 tooltip 窗口显示大图预览
- 保持文本项、文件项现有行为不变

本次设计基于当前仓库现状修订，目标是明确真正待实现的链路，避免重复改动已经存在的图片存储与详情字段。

## 当前现状

- 后端已经支持图片剪贴项落库，包含图片存储、宽高、格式、文件大小等元数据
- `ClipItemDetail` 已包含图片字段，图片真实文件以相对应用数据目录的路径存储，例如 `images/xxx.png`
- Picker 当前只把图片项当作普通文本摘要渲染，tooltip 也只显示文本内容
- `ClipItemSummary` 以及对应的 Rust `ClipItemSummary` 仍未暴露图片字段，因此列表层拿不到缩略图所需的数据
- `list_recent`、`list_favorites`、搜索结果的 summary 查询当前未选出图片列
- mock 明细数据中已经有图片样例，但 `toSummary()` 没有把图片字段透出
- tooltip 宿主页当前在插入 HTML 后立即测量窗口尺寸；如果直接插入 `<img>`，会因为图片异步加载导致首次尺寸不准确

## 设计目标

1. Picker 列表无需额外拉取详情即可渲染图片缩略图
2. tooltip 复用现有原生窗口能力，但要正确处理图片异步加载后的尺寸测量
3. Tauri 运行时稳定显示图片缩略图与大图预览，浏览器 mock 模式稳定显示占位缩略图
4. 不引入新的数据库 migration

## 设计决策

### 1. 数据层：补齐 Summary 合约

前端和 Rust 的 `ClipItemSummary` 都新增以下字段：

```typescript
interface ClipItemSummary {
  // ...现有字段
  imagePath: string | null;
  imageWidth: number | null;
  imageHeight: number | null;
  imageFormat: string | null;
  fileSize: number | null;
}
```

说明：

- `imagePath` 用于缩略图和大图预览的数据来源
- `imageWidth`、`imageHeight` 用于 tooltip 元信息与尺寸兜底
- `imageFormat` 用于 tooltip 元信息展示
- `fileSize` 本次不是必需显示项，但补进 summary 后可避免后续再次扩字段

后端改动点：

- Rust `ClipItemSummary` 同步增加以上字段
- `list_recent`、`list_favorites`、搜索 summary 查询补充 `image_path, image_width, image_height, image_format, file_size`
- `map_summary_row` 负责把这些字段映射到 summary
- 数据库表已有相关列，无需 migration

### 2. 图片 URL 解析：不要直接在前端对相对路径调用 `convertFileSrc`

当前 `image_path` 存储的是相对应用数据目录的相对路径，例如 `images/demo.png`。前端不能假设这个值本身就是可直接传给 `convertFileSrc()` 的绝对文件路径。

本次采用两步解析策略，在 Rust 侧完成路径解析与安全校验，在前端侧完成 URL 转换：

**Rust 侧：新增 Tauri 命令 `resolve_image_path`**

```rust
#[tauri::command]
fn resolve_image_path(image_path: String, app: AppHandle, state: State<AppState>) -> Result<String, String> {
    // 1. 从 AppState 获取 ImageStorage 实例（复用应用启动时的基准目录）
    // 2. 调用 ImageStorage::resolve_image_path 做路径遍历防护 + 拼接绝对路径
    // 3. 检查文件是否存在
    // 4. 返回绝对路径字符串
}
```

安全性说明：

- 复用 `ImageStorage` 实例，其基准目录在应用启动时由 `resolve_app_data_dir()` 确定（包含 `app_data_dir` 不可用时的 `current_dir()/.floatpaste-data` 回退），确保预览阶段与入库阶段使用同一基准目录
- `resolve_image_path` 内部已有路径遍历防护（拒绝 `..`、根路径）
- 文件存在性检查防止已删除图片的幽灵路径
- 当前仓库中的 `ImageStorage::resolve_image_path` 还是私有方法；本次实现需要将其提升为可复用的 `pub(crate)` 方法，或新增等价的公开包装方法，避免在命令层复制一份路径校验逻辑

**前端侧：使用 `convertFileSrc` 将绝对路径转为 webview 可用 URL**

```typescript
// bridge 层
async function getImageUrl(imagePath: string): Promise<string | null> {
  // Tauri 运行时：调用 resolve_image_path 获取绝对路径，再用 convertFileSrc 转换
  // 浏览器 mock 模式：直接返回占位图 data URL
}
```

约束：

- bridge 层负责运行时分支，组件层不直接拼接应用数据目录
- 如果路径无效、文件不存在或解析失败，返回 `null`，前端回退到无图占位
- `resolve_image_path` 是无副作用的纯路径计算命令，不涉及文件 I/O（除了存在性检查）

### 3. 列表项渲染：图片项显示缩略图

图片项在列表中的渲染方式：

- 在 `contentPreview` 左侧显示 32 × 32 px 缩略图
- 缩略图使用 `<img>`，样式为 `object-fit: cover`
- 右侧保留现有文本摘要，继续显示 `contentPreview`
- 非图片项保持不变

渲染规则：

- 仅当 `item.type === "image"` 且成功拿到图片 URL 时显示缩略图
- 缩略图加载失败时回退为现有文字样式，不阻塞列表渲染
- 不为列表项引入额外的异步详情查询

**列表缩略图的 URL 获取策略：**

- 列表数据返回后，组件渲染时**按需异步获取**图片 URL
- 每个图片项首次渲染时调用 `getImageUrl`，结果缓存在组件级 `Map<itemId, imageUrl>` 中
- 避免列表滚动或重新渲染时重复调用
- `resolve_image_path` 是本地路径计算，50 条记录中即使全部是图片项，总耗时也在毫秒级
- 如果未来需要进一步优化，可改为 Rust 侧在 summary 中直接返回绝对路径（此时 `getImageUrl` 退化为纯前端的 `convertFileSrc` 调用）

### 4. 悬浮大图预览：复用原生 tooltip，但补齐异步测量方案

继续复用现有 `showTooltip` 原生命令，但图片 tooltip 需要单独分支。

图片 tooltip HTML 结构：

```html
<div class="tooltip-image-preview">
  <img src="{imageUrl}" alt="" />
</div>
<div class="tooltip-meta">
  <span class="meta-badge">图片</span>
  <span class="meta-size">{width} × {height}</span>
  <span class="meta-format">{format}</span>
  <span class="meta-source">{sourceApp}</span>
</div>
```

尺寸策略：

- 预览区域最大显示尺寸为 320 × 240 px
- 保持原始宽高比，使用 `max-width`、`max-height`、`object-fit: contain`
- 原图更小时按原始尺寸显示，不强制放大

关键实现约束：

- `public/tooltip.html` 当前是在 `innerHTML` 后立即测量尺寸，不能假设图片已加载完成
- 图片 tooltip 采用**两阶段测量**策略，复用现有 `tooltip_ready` 通道，但必须把“待显示状态”从单纯坐标扩展为带 requestId 的请求上下文
- 图片 tooltip 在图片 `load`/`error`/超时三者之一发生前保持窗口不可见；不再采用“先显示最小尺寸，再二次放大”的策略

**两阶段测量的时序设计：**

1. `showTooltip` 被调用，前端生成 `requestId`；Rust 侧保存 `{ requestId, x, y }` 到待显示状态
2. tooltip 宿主页收到 HTML 与 `requestId`，先清理上一个请求残留的 `<img>` 监听器、超时器与本地状态，再设置 `innerHTML`
3. 图片 tooltip 不立即调用 `tooltip_ready`，而是在 `<img>` 上监听 `load`/`error` 并启动 2 秒超时保护
4. 图片加载完成后触发 `load` 事件，此时调用 `tooltip_ready({ requestId, width, height })`
5. 如果图片加载失败（`error`）或超时，回退为纯文本 tooltip，然后调用 `tooltip_ready({ requestId, width, height })`
6. 如果在图片完成前用户已移开鼠标，`hideTooltip` 需要同时清除 Rust 侧待显示状态和 tooltip.html 内的监听器/超时器
7. 任何迟到的 `load`/`error`/超时回调，如果 `requestId` 已过期，前端直接丢弃；若仍意外进入 Rust 侧，Rust 也必须在 requestId 不匹配时静默丢弃，不写日志

**为什么不在 innerHTML 后立即调用一次 `tooltip_ready`：**

当前 `on_tooltip_ready` 会 `take()` 消费 `PENDING_TOOLTIP_POS`，第二次调用位置数据已丢失。如果没有 requestId 防护，旧图片的迟到回调还可能消费新 hover 的位置。改为**图片 tooltip 只在图片加载完成后才调 `tooltip_ready`，并携带 requestId 做双端校验**；文本 tooltip 保持原有立即调用逻辑，但同样复用 requestId 通道以统一竞态处理。

**视觉跳变规避：**

- tooltip 窗口在 `showTooltip` 时保持 `visible: false`（当前已如此）
- 图片 tooltip 的 `tooltip_ready` 在图片加载后才触发，此时窗口才变为可见
- 用户感知不到"小→大"的跳变，因为窗口从未以小尺寸出现过
- 首次可见时窗口尺寸已经包含图片实际大小

**图片加载超时保护：**

- 为 `<img>` 设置 2 秒加载超时，超时后视为加载失败，回退纯文本 tooltip
- tooltip.html 侧调用 `invoke('tooltip_ready')` 时使用 `.catch(() => {})` 静默吞掉 rejected promise（Rust 侧在无待处理位置时返回错误），避免正常交互下产生控制台噪音
- tooltip.html 在新的 `showTooltip` 到来或 `hideTooltip` 调用时，必须清理上一次请求注册的 `load`/`error` 监听器与超时器，避免旧请求串扰新请求

交互规则：

- 与现有文本 tooltip 保持一致，鼠标悬浮 100ms 后触发
- 鼠标离开时立即隐藏
- 图片项不再显示纯文本 tooltip，而是显示图片预览 tooltip

**tooltip 异步流程改造：**

当前 `handleItemMouseMove`（`PickerShell.tsx`）在鼠标移动时同步构建 `tooltipHtml` 并延迟 100ms 发送。图片 tooltip 需要异步获取图片 URL，流程改造如下：

```
鼠标悬浮 → setTimeout(100ms) → 记录当前 requestId →
  if (图片项) {
    getImageUrl(item.imagePath) →
      // 异步返回后，二次校验 requestId 是否仍匹配当前悬浮项
      if (requestId !== tooltipRequestIdRef.current) return; // 用户已移开或移到别的条目，丢弃过期结果
      成功: 构建 img HTML → showTooltip(requestId, ...)
      失败: 回退纯文本 HTML → showTooltip(requestId, ...)
  } else {
    同步构建文本 HTML → showTooltip(requestId, ...)（与现在一致）
  }
```

- 图片 URL 获取在 100ms 延迟回调中发起，用户感知的延迟 = 100ms + 路径解析开销
- `getImageUrl` 返回后必须再次校验 `tooltipRequestIdRef`，防止用户在异步等待期间移到别的条目时显示过期图片
- `resolve_image_path` 是本地路径计算，耗时可忽略（< 1ms），对用户几乎无感
- `getImageUrl` 的结果可以按 item 缓存，同一 item 重复悬浮不重复调用
- `showTooltip` / `tooltip_ready` / tooltip.html 内部状态三处都必须共享同一个 `requestId`，避免“旧图片迟到回调覆盖新 tooltip” 的竞态

### 5. HTML 安全与失败回退

由于 tooltip 宿主页当前使用 `innerHTML` 注入内容，本次实现必须显式处理以下问题：

- 文本节点继续使用现有 HTML 转义
- `src`、`data-*` 等属性值必须做属性级转义，不能直接插值原始字符串
- 图片 URL 为空或加载失败时，tooltip 回退为纯文本摘要，不显示破图
- mock 使用 data URL 时也必须经过相同的属性转义逻辑

### 6. Mock 数据与浏览器预览

mock 层需要补齐两部分：

- `ClipItemSummary` mock 返回图片字段
- 浏览器预览模式提供可显示的占位图 URL

边界说明：

- 浏览器 mock 模式本次仅要求验证列表缩略图链路，不额外实现浏览器版悬浮大图 tooltip
- `showTooltip` / `hideTooltip` 在非 Tauri 环境继续保持 no-op，避免为了文档预览引入第二套 tooltip 实现
- 若后续需要浏览器里也预览大图，应单独设计 DOM tooltip 方案，而不是混入本次 Tauri 原生窗口改造

具体要求：

- 现有 `demo-3` 图片样例继续保留
- `toSummary()` 需要把 `imagePath`、`imageWidth`、`imageHeight`、`imageFormat`、`fileSize` 一并带出
- 浏览器模式下不依赖真实本地文件，统一使用占位图
- mock 占位图使用内联 SVG data URL，无需引入额外静态资源：

```typescript
const MOCK_IMAGE_URL = `data:image/svg+xml,${encodeURIComponent(
  '<svg xmlns="http://www.w3.org/2000/svg" width="200" height="150" fill="%23666"><rect width="200" height="150" rx="8"/><text x="100" y="82" text-anchor="middle" fill="%23ccc" font-size="14">图片预览</text></svg>'
)}`;
```

## 变更范围

| 层 | 文件 | 变更 |
|---|---|---|
| TS 类型 | `src/shared/types/clips.ts` | `ClipItemSummary` 增加图片字段 |
| Rust 类型 | `src-tauri/src/domain/clip_item.rs` | `ClipItemSummary` struct 增加图片字段 |
| Rust 查询 | `src-tauri/src/repository/sqlite_repository.rs` | `list_recent`、`list_favorites`、`search_recent`、`search_with_keyword` 的 summary 查询与 `map_summary_row` 补齐图片列 |
| 前端渲染 | `src/features/picker/PickerShell.tsx` | 图片项渲染缩略图 |
| 前端 tooltip | `src/features/picker/PickerShell.tsx` | `buildTooltipHtml` 增加图片分支，`handleItemMouseMove` 改造为异步流程 |
| Tooltip 宿主 | `public/tooltip.html` | 图片加载后的延迟测量（两阶段策略）与样式支持 |
| Bridge 工具 | `src/bridge/` | `getImageUrl()` 及对应桥接逻辑 |
| Tauri 命令 | `src-tauri/src/commands/` | 新增 `resolve_image_path` 命令 |
| Tauri 服务 | `src-tauri/src/services/` | 路径解析服务逻辑与 tooltip requestId 竞态防护 |
| 图片存储 | `src-tauri/src/services/image_storage.rs` | 暴露可复用的安全路径解析入口，避免命令层重复实现 |
| 命令注册 | `src-tauri/src/lib.rs` | `invoke_handler` 中注册 `resolve_image_path` |
| Mock 数据 | `src/bridge/mockBackend.ts` | summary 补齐图片字段与占位图回退 |

## 验收标准

- Picker 列表中的图片项能看到稳定的 32 × 32 缩略图
- Tauri 桌面模式下，悬浮图片项可显示大图预览，首次打开不会出现明显裁切或尺寸错误
- 浏览器 mock 模式下，图片项能显示占位缩略图，且不会因图片预览逻辑报错
- 文本项、文件项现有 tooltip 行为不回归
- 图片 URL 解析失败时，列表与 tooltip 都有明确回退，不出现异常日志刷屏
- 快速划过多张图片项时，不会出现旧图片的迟到回调覆盖新 tooltip 的现象

## 不包含

- 图片编辑功能
- 图片预览中的缩放、旋转等交互
- 点击放大到独立窗口
- 图片搜索优化，例如按尺寸或格式过滤
