import { gzipSync } from 'node:zlib';
import { defineConfig, loadEnv, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';

const MAX_RAW_WASM_BYTES = 20 * 1024 * 1024; // Keep every static asset well under Cloudflare's 25 MiB limit.

/**
 * Gzip-compress oversized .wasm assets (e.g. ffmpeg-core.wasm, ~31 MiB -> ~9.8 MiB)
 * during production builds and rewrite references in emitted JS chunks to .wasm.gz.
 * At runtime, @refract/engine-wasm transparently decompresses .wasm.gz via DecompressionStream.
 */
function compressLargeWasm(): Plugin {
  return {
    name: 'refract-compress-large-wasm',
    apply: 'build',
    enforce: 'post',
    generateBundle(_options, bundle) {
      const renamed = new Map<string, string>();
      for (const [fileName, item] of Object.entries(bundle)) {
        if (item.type !== 'asset' || !fileName.endsWith('.wasm')) continue;
        const source = typeof item.source === 'string' ? Buffer.from(item.source) : Buffer.from(item.source);
        if (source.byteLength <= MAX_RAW_WASM_BYTES) continue;

        const gzName = `${fileName}.gz`;
        const compressed = gzipSync(source, { level: 9 });
        delete bundle[fileName];
        this.emitFile({
          type: 'asset',
          fileName: gzName,
          source: compressed,
        });
        renamed.set(fileName, gzName);
      }

      if (renamed.size === 0) return;

      for (const item of Object.values(bundle)) {
        if (item.type !== 'chunk') continue;
        for (const [oldName, newName] of renamed) {
          const oldBase = oldName.split('/').pop()!;
          const newBase = newName.split('/').pop()!;
          if (item.code.includes(oldName)) {
            item.code = item.code.replaceAll(oldName, newName);
          } else if (item.code.includes(oldBase)) {
            item.code = item.code.replaceAll(oldBase, newBase);
          }
        }
      }
    },
  };
}

const isolationHeaders = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
};

export default defineConfig(({ mode }) => {
  // Load env files so VITE_BASE can be set for subpath hosting (e.g. GitHub Pages at /refract/).
  const env = loadEnv(mode, process.cwd(), '');

  return {
    plugins: [react(), compressLargeWasm()],
    // base supports subpath hosting. Defaults to '/' for root-domain hosting. Set VITE_BASE=/refract/
    // for GitHub Pages user/organization sites or any host that serves the app under a path prefix.
    base: env.VITE_BASE ?? '/',
    // WASM packages load their .wasm assets via new URL(…, import.meta.url); pre-bundling breaks those paths in dev mode.
    optimizeDeps: {
      exclude: [
        '@jsquash/jpeg',
        '@jsquash/png',
        '@jsquash/webp',
        '@jsquash/resize',
        'libheif-js',
        '@neslinesli93/qpdf-wasm',
        '@ffmpeg/ffmpeg',
        '@ffmpeg/core',
      ],
    },
    worker: {
      format: 'es',
      plugins: () => [compressLargeWasm()],
    },
    server: {
      headers: isolationHeaders,
    },
    preview: {
      port: 4173,
      strictPort: true,
      headers: isolationHeaders,
    },
    build: {
      outDir: 'dist',
      assetsDir: 'assets',
      // Keep sourcemaps off for smaller production bundles; enable for debugging if needed.
      sourcemap: false,
      // .wasm files are lazy-loaded by Web Workers via ?url imports; ensure they land in dist/assets.
      rollupOptions: {
        output: {
          entryFileNames: `assets/[name]-[hash].js`,
          chunkFileNames: `assets/[name]-[hash].js`,
          assetFileNames: `assets/[name]-[hash].[ext]`,
        },
      },
    },
  };
});
