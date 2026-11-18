import { defineConfig } from 'cf/config';

export default defineConfig({
  worker: {
    name: 'refract',
    compatibilityDate: '2026-11-18',
    domains: ['refract.advaitachandra.in'],
    assets: {
      notFoundHandling: 'single-page-application',
      htmlHandling: 'auto-trailing-slash',
    },
    observability: {
      enabled: true,
      traces: {
        enabled: true,
      },
    },
  },
});
