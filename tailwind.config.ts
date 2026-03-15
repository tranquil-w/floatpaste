import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      colors: {
        // Catppuccin 配色 (通过 CSS 变量动态切换)
        cp: {
          rosewater: "var(--cp-rosewater)",
          flamingo: "var(--cp-flamingo)",
          pink: "var(--cp-pink)",
          mauve: "var(--cp-mauve)",
          red: "var(--cp-red)",
          maroon: "var(--cp-maroon)",
          peach: "var(--cp-peach)",
          yellow: "var(--cp-yellow)",
          green: "var(--cp-green)",
          teal: "var(--cp-teal)",
          sky: "var(--cp-sky)",
          sapphire: "var(--cp-sapphire)",
          blue: "var(--cp-blue)",
          lavender: "var(--cp-lavender)",
          text: "var(--cp-text)",
          subtext1: "var(--cp-subtext1)",
          subtext0: "var(--cp-subtext0)",
          overlay2: "var(--cp-overlay2)",
          overlay1: "var(--cp-overlay1)",
          overlay0: "var(--cp-overlay0)",
          surface2: "var(--cp-surface2)",
          surface1: "var(--cp-surface1)",
          surface0: "var(--cp-surface0)",
          base: "var(--cp-base)",
          mantle: "var(--cp-mantle)",
          crust: "var(--cp-crust)",
        },
        // 兼容性别名
        ink: "var(--cp-text)",
        paper: "var(--cp-base)",
        accent: "var(--cp-lavender)",
        accentDeep: "var(--cp-blue)",
        moss: "var(--cp-green)",
        primaryDark: "var(--cp-crust)",
        // 状态颜色
        success: "var(--cp-green)",
        warning: "var(--cp-yellow)",
        error: "var(--cp-red)",
        info: "var(--cp-sapphire)",
      },
      boxShadow: {
        panel: "0 24px 80px rgba(var(--cp-text-rgb), 0.12)",
        "panel-dark": "0 24px 80px rgba(0, 0, 0, 0.55)",
      },
      fontFamily: {
        display: ["Georgia", "serif"],
        body: ["'Segoe UI'", "sans-serif"],
      },
    },
  },
  plugins: [],
} satisfies Config;
