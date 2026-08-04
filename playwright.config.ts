import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: ['*.spec.ts'],
  timeout: 60000,
  use: {
    channel: 'msedge',
    headless: true,
  },
});