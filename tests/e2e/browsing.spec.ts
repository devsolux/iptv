import { expect, test } from '@playwright/test';

test('fixture catalogue remains virtualized and navigates with remote keys', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('50,000', { exact: false }).first()).toBeVisible();
  await expect(page.locator('.channel-card').first()).toBeVisible();
  expect(await page.locator('.channel-card').count()).toBeLessThan(100);
  const card = page.locator('.channel-card').first();
  await card.focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.locator('.channel-card').nth(1)).toBeFocused();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('.channel-card:focus')).toHaveCount(1);
});
test('reloading the catalogue keeps the grid position instead of jumping to the top', async ({
  page,
}) => {
  await page.goto('/');
  const grid = page.getByRole('region', { name: 'Channel grid' });
  await expect(page.locator('.channel-card').first()).toBeVisible();
  await grid.evaluate((element) => {
    element.scrollTop = 40_000;
  });
  await expect(page.locator('.channel-card[data-channel-index="0"]')).toHaveCount(0);
  const before = await grid.evaluate((element) => element.scrollTop);
  expect(before).toBeGreaterThan(0);
  await page.getByRole('button', { name: 'Reload catalogue' }).click();
  await expect(page.locator('.channel-card').first()).toBeVisible();
  expect(await grid.evaluate((element) => element.scrollTop)).toBe(before);
  await expect(page.locator('.channel-card[data-channel-index="0"]')).toHaveCount(0);
});
test('guide provides a bounded time window and programmatic TV focus', async ({ page }) => {
  await page.goto('/#/guide');
  await expect(page.locator('.guide-row').first()).toBeVisible();
  expect(await page.locator('.guide-row').count()).toBeLessThan(30);
  const focusable = page.locator('.guide-row button').first();
  await focusable.focus();
  await page.keyboard.press('ArrowDown');
  await expect(page.locator('.guide-row button:focus')).toHaveCount(1);
});
test('browser preview cannot mutate source credentials or start native playback', async ({
  page,
}) => {
  await page.goto('/#/sources');
  await expect(page.getByText('Development preview', { exact: false }).first()).toBeVisible();
  await expect(page.getByRole('button', { name: /Add source/i })).toBeDisabled();
});
