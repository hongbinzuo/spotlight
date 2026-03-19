import { defineConfig } from "vite";

export default defineConfig({
  server: {
    host: "127.0.0.1",
    port: 1421,
    strictPort: true
  },
  preview: {
    host: "127.0.0.1",
    port: 1421,
    strictPort: true
  },
  build: {
    outDir: "dist",
    emptyOutDir: true
  }
});

