import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        ink: "#111827",
        paper: "#f6f0e7",
        accent: "#d97706",
        accentDeep: "#92400e",
        moss: "#3f6212",
      },
      boxShadow: {
        panel: "0 24px 80px rgba(17, 24, 39, 0.12)",
      },
      fontFamily: {
        display: ["Georgia", "serif"],
        body: ["'Segoe UI'", "sans-serif"],
      },
    },
  },
  plugins: [],
} satisfies Config;
