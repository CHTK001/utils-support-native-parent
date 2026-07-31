import { test, expect } from '@playwright/test';

const EXPECTED_PORT = process.env.FRONTEND_PORT || '8848';
const FRONTEND = 'http://192.168.50.101:' + EXPECTED_PORT;

test('frontend metadata', async ({ page }) => {
  const resp = await page.goto(FRONTEND, { timeout: 20000 });
  expect(resp?.ok()).toBeTruthy();
  const title = await page.title();
  console.log('title', title);
  expect(title).toBeTruthy();
});
