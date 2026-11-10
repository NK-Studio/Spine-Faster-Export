import { defineConfig } from "vite";

// Tauri 개발 서버: 고정 포트, 오류 덮개 대신 콘솔.
export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] } },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 2000 },
});
