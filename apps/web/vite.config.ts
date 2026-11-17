import { defineConfig, loadEnv } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig(({ mode }) => {
  // Load env files so VITE_BASE can be set for subpath hosting (e.g. GitHub Pages at /refract/).
  const env = loadEnv(mode, process.cwd(), '');

  return {
    plugins: [react()],
    // base supports subpath hosting. Defaults to '/' for root-domain hosting. Set VITE_BASE=/refract/
    // for GitHub Pages user/organization sites or any host that serves the app under a path prefix.
    base: env.VITE_BASE ?? '/',
    // jSquash loads its .wasm via new URL(…, import.meta.url); pre-bundling would break that path.
    optimizeDeps: { exclude: ['@jsquash/jpeg'] },
    worker: { format: 'es' },
    preview: { port: 4173, strictPort: true },
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
