import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
  root: "apps/desktop",
  cacheDir: process.env.CERULEAN_VITE_CACHE_DIR || "../../.temp/build-cache/manual/vite",
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  build: {
    outDir: "../../dist",
    emptyOutDir: true,
    rolldownOptions: {
      output: {
        codeSplitting: {
          // Keep the framework runtime separate from frequently changing product UI.
          groups: [{ name: "react-vendor", test: /[\\/]node_modules[\\/](?:react|react-dom|scheduler)[\\/]/ }],
        },
      },
    },
  },
});
