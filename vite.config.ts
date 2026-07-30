import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  // Tauri 自行管理终端输出，关闭 Vite 的清屏以免打断。
  clearScreen: false,
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 3000,
    strictPort: true,
    watch: {
      // 忽略 Rust 后端改动，避免触发前端无谓的 HMR 重载。
      ignored: ["**/src-tauri/**"],
    },
  },
});
