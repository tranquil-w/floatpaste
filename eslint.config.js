import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";
import prettier from "eslint-config-prettier";

export default tseslint.config(
  // 全局忽略
  {
    ignores: ["dist/**", "src-tauri/**", "scripts/**", "tests/**"],
  },

  // 基础推荐规则
  js.configs.recommended,

  // TypeScript 推荐规则（非类型检查版，类型问题交给 tsc --noEmit）
  ...tseslint.configs.recommended,

  // React hooks 规则
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: {
      "react-hooks": reactHooks,
    },
    languageOptions: {
      ecmaVersion: 2021,
      sourceType: "module",
      globals: {
        window: "readonly",
        document: "readonly",
        console: "readonly",
        navigator: "readonly",
        setTimeout: "readonly",
        clearTimeout: "readonly",
        setInterval: "readonly",
        clearInterval: "readonly",
        requestAnimationFrame: "readonly",
        cancelAnimationFrame: "readonly",
        ResizeObserver: "readonly",
        Intl: "readonly",
        HTMLElement: "readonly",
        HTMLDivElement: "readonly",
        HTMLButtonElement: "readonly",
        HTMLInputElement: "readonly",
        HTMLTextAreaElement: "readonly",
        Event: "readonly",
        MouseEvent: "readonly",
        KeyboardEvent: "readonly",
        Node: "readonly",
        DOMParser: "readonly",
        localStorage: "readonly",
      },
    },
    rules: {
      ...reactHooks.configs.recommended.rules,
      // 以下规则在现有代码中存在有意为之的模式（如 ref 镜像 state、
      // effect 内初始化监听器），降为 warn 以提示但不阻断构建。
      "react-hooks/set-state-in-effect": "warn",
      "react-hooks/exhaustive-deps": "warn",
      "react-hooks/refs": "warn",
    },
  },

  // 关闭与 Prettier 冲突的格式化规则
  prettier,
);
