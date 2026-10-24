import { defineConfig } from "vitest/config";
import preact from "@preact/preset-vite";

// Relative base so the page works from any sub-path (GitHub Pages project sites).
export default defineConfig({
  base: "./",
  plugins: [preact()],
  build: { target: "es2022" },
  test: { include: ["src/**/*.test.ts"] },
});
