import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vitest/config";
import { viteSingleFile } from "vite-plugin-singlefile";

export default defineConfig({
  plugins: [svelte(), viteSingleFile()],
  build: {
    outDir: process.env.CELERITY_UI_OUT_DIR ?? "../assets",
    emptyOutDir: true,
    cssCodeSplit: false,
    minify: "terser",
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});
