import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Dev server proxies /api to the axum backend (RECON_WEB_ADDR, default
// 127.0.0.1:3000) so the browser only ever talks to one origin — no CORS
// needed in dev or prod. `npm run build` emits to dist/, which recon-web
// embeds into the binary via rust-embed for a single-binary deploy.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    proxy: {
      "/api": {
        target: process.env.RECON_API_PROXY_TARGET ?? "http://127.0.0.1:3000",
        changeOrigin: true,
      },
      "/reports": {
        target: process.env.RECON_API_PROXY_TARGET ?? "http://127.0.0.1:3000",
        changeOrigin: true,
      },
    },
  },
});
