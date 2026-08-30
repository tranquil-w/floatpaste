import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  darkMode: "class",
  theme: {
    extend: {
      transitionDuration: {
        '250': '250ms',
      },
      colors: {
        // Primer semantic tokens (via CSS variables)
        pg: {
          "fg-default": "var(--pg-fg-default)",
          "fg-muted": "var(--pg-fg-muted)",
          "fg-subtle": "var(--pg-fg-subtle)",
          "fg-on-emphasis": "var(--pg-fg-on-emphasis)",
          "accent-fg": "var(--pg-accent-fg)",
          "accent-emphasis": "var(--pg-accent-emphasis)",
          "accent-subtle": "var(--pg-accent-subtle)",
          "accent-hover": "var(--pg-accent-hover)",
          "canvas-default": "var(--pg-canvas-default)",
          "canvas-subtle": "var(--pg-canvas-subtle)",
          "canvas-inset": "var(--pg-canvas-inset)",
          "border-default": "var(--pg-border-default)",
          "border-window": "var(--pg-border-window)",
          "border-muted": "var(--pg-border-muted)",
          "border-subtle": "var(--pg-border-subtle)",
          "border-accent": "var(--pg-border-accent)",
          "success-fg": "var(--pg-success-fg)",
          "success-emphasis": "var(--pg-success-emphasis)",
          "success-subtle": "var(--pg-success-subtle)",
          "danger-fg": "var(--pg-danger-fg)",
          "danger-emphasis": "var(--pg-danger-emphasis)",
          "danger-subtle": "var(--pg-danger-subtle)",
          "warning-fg": "var(--pg-warning-fg)",
          "warning-emphasis": "var(--pg-warning-emphasis)",
          "warning-subtle": "var(--pg-warning-subtle)",
          "done-fg": "var(--pg-done-fg)",
          "done-emphasis": "var(--pg-done-emphasis)",
          "done-subtle": "var(--pg-done-subtle)",
          favorite: "var(--pg-favorite)",
        },
      },
      boxShadow: {
        'pg-sm': "var(--pg-shadow-sm)",
        'pg-md': "var(--pg-shadow-md)",
        'pg-lg': "var(--pg-shadow-lg)",
        'pg-xl': "var(--pg-shadow-xl)",
      },
      fontFamily: {
        display: ["Georgia", "serif"],
        body: ["'-apple-system'", "BlinkMacSystemFont", "'Segoe UI'", "'Noto Sans'", "Helvetica", "Arial", "sans-serif"],
      },
    },
  },
  plugins: [],
} satisfies Config;
