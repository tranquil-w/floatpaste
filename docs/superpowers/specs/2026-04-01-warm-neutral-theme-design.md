# 暖调中性色主题（浅色）

## 背景

浅色主题使用 Primer 默认中性色（如 `#EFF2F5`），在冷色调显示器上呈现泛蓝的冷色感。用户偏好暖色调，将浅色主题调整为暖调中性色，呈现纸质底色感。

暗色主题保持冷蓝灰色调不变，与浅色形成冷暖对比。

## 方案

修改 `src/index.css` 中的 neutral 色阶（浅色 `:root`），以及对应的 `--pg-shadow-color`。
浅色模式的 `--pg-accent-subtle` 也调整为暖色调，使选中态/hover 态与暖灰底色协调。

暗色 neutral 色阶和语义色（blue/green/yellow/orange/red/purple）保持不变。
主题切换机制、Tailwind 配置不变。

## 颜色值

### 浅色（暖灰，纸质底色感）

| Token | 冷色原值 | 暖色新值 |
|-------|---------|---------|
| neutral-0 | `#ffffff` | `#FFFEFA` |
| neutral-1 | `#F6F8FA` | `#FAF8F4` |
| neutral-2 | `#EFF2F5` | `#F3F0EB` |
| neutral-3 | `#E6EAEF` | `#EBE7E1` |
| neutral-4 | `#E0E6EB` | `#E4DFD8` |
| neutral-5 | `#DAE0E7` | `#DFD9D1` |
| neutral-6 | `#D1D9E0` | `#D5CFC6` |
| neutral-7 | `#C8D1DA` | `#CCC5BB` |
| neutral-8 | `#818B98` | `#868078` |
| neutral-9 | `#59636E` | `#5D574F` |
| neutral-10 | `#454C54` | `#49433B` |
| neutral-11 | `#393F46` | `#3D3730` |
| neutral-12 | `#25292E` | `#2A241D` |
| neutral-13 | `#1f2328` | `#241E17` |

Shadow: `31, 35, 40` → `36, 30, 23`

Accent-subtle: 原 `var(--pg-blue-0)` → 暖色值 `#f5eddc`

### 暗色（保持冷蓝灰色调，不变）

暗色主题保持原有冷蓝灰色调，不做修改。

## 实现步骤

1. 替换 `src/index.css` 中 `:root` 的 neutral-0 ~ neutral-13 和 shadow-color
2. 在 `:root` 中将 `--pg-accent-subtle` 从 `var(--pg-blue-0)` 改为暖色值
3. 恢复 `--pg-blue-0/1/2` 为蓝色原值（不再用于 accent-subtle）
4. 运行 `pnpm build` 验证
5. 启动 `pnpm tauri dev` 视觉确认浅色主题

## 验证标准

- 浅色背景呈现明显的纸质暖感
- 浅色选中态/hover 态的暖黄色与暖灰底色协调
- 暗色主题视觉不变
- 文字对比度未降低，可读性正常
- 语义色（链接蓝、成功绿等）不受影响
