import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // jSquash loads its .wasm via new URL(…, import.meta.url); pre-bundling would break that path.
  optimizeDeps: { exclude: ['@jsquash/jpeg'] },
  worker: { format: 'es' },
  preview: { port: 4173, strictPort: true },
});
