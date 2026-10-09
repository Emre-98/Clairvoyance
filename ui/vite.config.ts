import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import JavaScriptObfuscator from "javascript-obfuscator";

// Release builds: light obfuscation of the final, minified chunks, so the UI code shipped inside the
// app is harder to read and reuse (the source is private). Light on purpose (PLAN.md performance
// rules): no control-flow flattening, dead code, self-defending or anything needing eval (the CSP
// forbids it); only renamed identifiers and the string literals moved into a shuffled lookup array.
// `CV_NO_OBFUSCATE=1 npm run build` builds without it (for debugging a release build).
function obfuscate(): Plugin {
  return {
    name: "cv-obfuscate",
    apply: "build",
    enforce: "post",
    generateBundle(_options, bundle) {
      if (process.env.CV_NO_OBFUSCATE === "1") return;
      for (const chunk of Object.values(bundle)) {
        if (chunk.type !== "chunk") continue;
        chunk.code = JavaScriptObfuscator.obfuscate(chunk.code, {
          target: "browser",
          sourceMap: false,
          compact: true,
          simplify: true,
          identifierNamesGenerator: "mangled-shuffled",
          renameGlobals: false,
          renameProperties: false,
          stringArray: true,
          stringArrayThreshold: 0.75,
          stringArrayRotate: true,
          stringArrayShuffle: true,
          stringArrayEncoding: [],
          stringArrayIndexShift: true,
          stringArrayWrappersCount: 1,
          stringArrayWrappersType: "variable",
          splitStrings: false,
          controlFlowFlattening: false,
          deadCodeInjection: false,
          numbersToExpressions: false,
          transformObjectKeys: false,
          selfDefending: false,
          debugProtection: false,
          disableConsoleOutput: false,
          unicodeEscapeSequence: false,
        }).getObfuscatedCode();
      }
    },
  };
}

export default defineConfig({
  plugins: [svelte(), obfuscate()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 800, sourcemap: false },
  esbuild: { legalComments: "none" },
});
