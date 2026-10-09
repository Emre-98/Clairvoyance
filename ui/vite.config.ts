import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import JavaScriptObfuscator from "javascript-obfuscator";

// Release builds obfuscate only the "cold" chunks: Settings and the first-run setup (loaded on
// demand, App.svelte) and the components only they use. The library, player, timeline and overlay
// (the code that runs every frame) are left exactly as they are, so playback costs nothing extra.
// No eval-based options (the CSP forbids eval). `CV_NO_OBFUSCATE=1 npm run build` turns it off.
const COLD = /\/src\/(pages\/(Settings|Setup)|components\/(BuiltinRecorder|GameModes|HotkeyInput|PerfTest|RuleSwitch))\.svelte$/;

function obfuscateCold(): Plugin {
  return {
    name: "cv-obfuscate-cold",
    apply: "build",
    enforce: "post",
    generateBundle(_options, bundle) {
      if (process.env.CV_NO_OBFUSCATE === "1") return;
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== "chunk" || chunk.isEntry) continue;
        const ids = chunk.moduleIds.map((id) => id.split("?")[0]).filter((id) => id.endsWith(".svelte"));
        if (!(chunk.facadeModuleId && COLD.test(chunk.facadeModuleId)) && !(ids.length > 0 && ids.every((id) => COLD.test(id)))) continue;
        chunk.code = JavaScriptObfuscator.obfuscate(chunk.code, {
          target: "browser",
          sourceMap: false,
          compact: true,
          simplify: true,
          identifierNamesGenerator: "mangled-shuffled",
          renameGlobals: false,
          stringArray: true,
          stringArrayThreshold: 1,
          stringArrayRotate: true,
          stringArrayShuffle: true,
          stringArrayEncoding: ["base64"],
          stringArrayWrappersCount: 2,
          stringArrayWrappersType: "function",
          splitStrings: true,
          splitStringsChunkLength: 8,
          controlFlowFlattening: true,
          controlFlowFlatteningThreshold: 0.5,
          deadCodeInjection: true,
          deadCodeInjectionThreshold: 0.2,
          numbersToExpressions: true,
          transformObjectKeys: true,
          selfDefending: false,
          debugProtection: false,
          disableConsoleOutput: false,
        }).getObfuscatedCode();
        this.info?.(`obfuscated ${chunk.fileName}`);
      }
    },
  };
}

export default defineConfig({
  plugins: [svelte(), obfuscateCold()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 800, sourcemap: false },
  esbuild: { legalComments: "none" },
});
