import { test, expect, type Page } from '../../apps/desktop/tests/harness';
import { installHost } from './mock-host';

const panes = (page: Page) => page.locator('.tab-content:not(.inactive) .session-pane');
async function ready(page: Page, options: Parameters<typeof installHost>[1] = {}) {
  await installHost(page, options); await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBe(1);
}
async function split(page: Page, number: number, direction: '向右分屏' | '向下分屏') {
  const count = await panes(page).count();
  await page.getByRole('button', { name: count === 1 ? '分屏' : `分屏窗格 ${number}`, exact: true }).click();
  await page.getByRole('menuitem', { name: direction, exact: true }).click();
  await expect(panes(page)).toHaveCount(count + 1);
  await expect(panes(page).last().locator('textarea')).toBeFocused();
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBeGreaterThanOrEqual(count + 1);
}
async function three(page: Page) { await split(page, 1, '向右分屏'); await split(page, 2, '向下分屏'); }
async function ids(page: Page): Promise<string[]> { return page.evaluate(() => (window as any).__testEvidence.prepared.map((p: any) => p.binding.sessionId)); }
async function close(page: Page, number: number) {
  await page.getByRole('button', { name: `关闭窗格 ${number}`, exact: true }).click();
  await page.getByRole('button', { name: '关闭窗格', exact: true }).click();
}
async function retainElements(page: Page) {
  await page.evaluate(() => { (window as any).__retainedTerminals = [...document.querySelectorAll('.session-pane .xterm')]; });
}
async function expectRetained(page: Page, count: number) {
  expect(await page.evaluate(() => (window as any).__retainedTerminals.every((el: HTMLElement) => el.isConnected))).toBe(true);
  expect(await page.evaluate(() => (window as any).__testEvidence.starts)).toBe(count);
  expect(await page.evaluate(() => (window as any).__testEvidence.events.filter((s: string) => s === 'view_failed'))).toEqual([]);
}

test('selected panes can be split recursively without remounting terminals or moving other regions', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message));
  await ready(page); await retainElements(page);
  const [firstId] = await ids(page);
  await page.evaluate(id => (window as any).__emitOutput(id, '\r\noriginal-buffer-survives\r\n'), firstId);
  await split(page, 1, '向右分屏');
  const left = await panes(page).nth(0).boundingBox();
  await split(page, 2, '向下分屏'); expect(await panes(page).nth(0).boundingBox()).toEqual(left);
  const rightTop = await panes(page).nth(1).boundingBox(); const rightBottom = await panes(page).nth(2).boundingBox();
  await retainElements(page); await split(page, 1, '向下分屏');
  expect(await panes(page).nth(1).boundingBox()).toEqual(rightTop);
  expect(await panes(page).nth(2).boundingBox()).toEqual(rightBottom);
  const topLeft = (await panes(page).nth(0).boundingBox())!; const bottomLeft = (await panes(page).nth(3).boundingBox())!;
  expect(topLeft.x).toBe(bottomLeft.x); expect(topLeft.y + topLeft.height).toBeLessThan(bottomLeft.y);
  await expect(panes(page).first().getByRole('listitem').filter({ hasText: 'original-buffer-survives' })).toBeVisible();
  await expectRetained(page, 4);
  expect(await page.evaluate(() => (window as any).__testEvidence.prepared[3].inheritSession)).toBe(firstId);
  expect(await page.locator('.current-pane:visible').count()).toBe(1);
  await page.screenshot({ path: 'test-results/nested-four-panes.png' });
  expect(errors).toEqual([]);
});

test('spatial shortcuts follow screen position rather than creation order and do not reach shells', async ({ page }) => {
  await ready(page); await three(page); await split(page, 1, '向下分屏');
  // Creation order is top-left, top-right, bottom-right, bottom-left.
  await page.evaluate(() => { (window as any).__testEvidence.inputBySession = []; });
  for (const [key, index] of [['Alt+ArrowRight', 2], ['Alt+ArrowUp', 1], ['Alt+ArrowLeft', 0], ['Alt+ArrowDown', 3], ['Alt+ArrowLeft', 3]] as const) {
    await page.keyboard.press(key); await expect(panes(page).nth(index).locator('textarea')).toBeFocused();
  }
  expect(await page.evaluate(() => (window as any).__testEvidence.inputBySession)).toEqual([]);
  await page.keyboard.type('selected-bottom-left');
  const sessionIds = await ids(page);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.inputBySession.at(-1)?.id)).toBe(sessionIds[3]);
  await page.keyboard.press('Control+Alt+W'); await expect(page.getByRole('dialog')).toContainText('关闭这个窗格');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await page.keyboard.press('Control+Shift+W'); await expect(page.getByRole('dialog')).toContainText('4 个窗格');
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await expect(panes(page)).toHaveCount(4);
});

test('closing a leaf collapses its parent locally; closing its uncle expands the retained subtree', async ({ page }) => {
  await ready(page, { outputEndDelayMs: 250, repeatFinishedState: true }); await three(page); await split(page, 1, '向下分屏');
  const sessionIds = await ids(page); const leftTop = await panes(page).nth(0).boundingBox(); const leftBottom = await panes(page).nth(3).boundingBox();
  await close(page, 2); await expect(panes(page)).toHaveCount(3);
  expect(await panes(page).nth(0).boundingBox()).toEqual(leftTop); expect(await panes(page).nth(2).boundingBox()).toEqual(leftBottom);
  await expect(panes(page).nth(1).locator('textarea')).toBeFocused();
  const right = (await panes(page).nth(1).boundingBox())!; expect(right.height).toBeGreaterThan(leftTop!.height * 2);
  const inner = page.getByRole('separator', { name: '调整窗格 1 大小', exact: true });
  await inner.focus(); await page.keyboard.press('ArrowDown'); const saved = await inner.getAttribute('aria-valuenow');
  await page.evaluate(() => { (window as any).__retainedTerminals = [...document.querySelectorAll('.session-pane')].filter(el => el.getAttribute('aria-label')?.match(/窗格 [13]：/)).map(el => el.querySelector('.xterm')); });
  await close(page, 2); await expect(panes(page)).toHaveCount(2);
  await expect(page.getByRole('separator')).toHaveAttribute('aria-orientation', 'horizontal');
  await expect(page.getByRole('separator')).toHaveAttribute('aria-valuenow', saved!);
  const first = (await panes(page).first().boundingBox())!; const second = (await panes(page).last().boundingBox())!;
  expect(first.width).toBe(1100); expect(second.width).toBe(1100);
  await expect(panes(page).first().locator('textarea')).toBeFocused();
  await expectRetained(page, 4);
  expect(await page.evaluate(() => (window as any).__testEvidence.forgotten)).toEqual([sessionIds[1], sessionIds[2]]);
});

test('nested dividers resize exactly their descendant terminals and retain ratios after window shrink', async ({ page }) => {
  await ready(page); await three(page);
  const sessionIds = await ids(page);
  const inner = page.getByRole('separator', { name: '调整窗格 2 大小', exact: true });
  // Wait for the layout caused by creation before collecting resize evidence.
  await expect.poll(() => page.evaluate(id => (window as any).__testEvidence.resize.some((r: any) => r.binding.sessionId === id), sessionIds[1])).toBe(true);
  await page.evaluate(() => { (window as any).__testEvidence.resize = []; });
  await inner.focus(); await page.keyboard.press('ArrowDown');
  await expect.poll(() => page.evaluate(() => [...new Set((window as any).__testEvidence.resize.map((r: any) => r.binding.sessionId))].sort())).toEqual([sessionIds[1], sessionIds[2]].sort());
  const ratio = await inner.getAttribute('aria-valuenow');
  const outer = page.getByRole('separator', { name: '调整窗格 1 大小', exact: true });
  await page.evaluate(() => { (window as any).__testEvidence.resize = []; });
  await outer.focus(); await page.keyboard.press('ArrowRight');
  await expect.poll(() => page.evaluate(() => [...new Set((window as any).__testEvidence.resize.map((r: any) => r.binding.sessionId))].sort())).toEqual([...sessionIds].sort());
  const outerRatio = await outer.getAttribute('aria-valuenow');
  await page.setViewportSize({ width: 480, height: 350 });
  await expect.poll(() => outer.getAttribute('aria-valuemin')).toBe('50');
  await page.setViewportSize({ width: 1100, height: 720 });
  await expect(inner).toHaveAttribute('aria-valuenow', ratio!); await expect(outer).toHaveAttribute('aria-valuenow', outerRatio!);
  expect(await page.evaluate(() => (window as any).__testEvidence.starts)).toBe(3);
  // Home/End must still leave enough actual terminal rows in the constrained child.
  await inner.focus(); await page.keyboard.press('Home');
  await expect.poll(() => page.evaluate(id => (window as any).__testEvidence.resize.filter((r: any) => r.binding.sessionId === id).at(-1)?.size.rows, sessionIds[1])).toBe(10);
  await page.keyboard.press('End');
  await expect.poll(() => page.evaluate(id => (window as any).__testEvidence.resize.filter((r: any) => r.binding.sessionId === id).at(-1)?.size.rows, sessionIds[2])).toBe(10);
});

test('splitting a zoomed pane uses its restored rectangle and only exits zoom on success', async ({ page }) => {
  await ready(page); await split(page, 1, '向右分屏');
  await page.keyboard.press('Control+Alt+Z'); await expect(panes(page).first()).toBeHidden();
  await page.keyboard.press('Control+Alt+D'); await expect(panes(page)).toHaveCount(3);
  await expect(page.getByRole('separator')).toHaveCount(2); await expect(panes(page).first()).toBeVisible();
  await expect(panes(page).last().locator('textarea')).toBeFocused();
  await page.keyboard.press('Control+Alt+Z'); await expect(panes(page).first()).toBeHidden();
  await page.getByRole('button', { name: '分屏', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: '向右分屏', exact: true })).toBeDisabled();
  await expect(page.getByRole('menuitem', { name: '向下分屏', exact: true })).toBeDisabled();
  await expect(page.getByRole('menu')).toContainText('40 列 × 10 行'); await page.keyboard.press('Escape');
  await panes(page).last().locator('textarea').focus(); await page.keyboard.press('Control+Alt+D');
  await expect(page.getByRole('status')).toContainText('空间不足');
  await expect(panes(page)).toHaveCount(3); await expect(panes(page).first()).toBeHidden();
  await page.keyboard.press('Control+Alt+Z'); await expect(panes(page).first()).toBeVisible();
  expect(await page.evaluate(() => (window as any).__testEvidence.starts)).toBe(3);
});

test('eight-pane cap is per tab, leaves room for closing and reopening, and does not bypass session budget', async ({ page }) => {
  await page.setViewportSize({ width: 2000, height: 1400 }); await ready(page, { maxSessions: 9 });
  await three(page); await split(page, 1, '向右分屏'); await split(page, 1, '向下分屏'); await split(page, 4, '向下分屏');
  await split(page, 2, '向右分屏'); await split(page, 3, '向右分屏');
  await page.getByRole('button', { name: '分屏窗格 8', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: '向右分屏', exact: true })).toBeDisabled();
  await expect(page.getByRole('menuitem', { name: '向下分屏', exact: true })).toBeDisabled();
  await expect(page.getByRole('menu')).toContainText('最多 8 个窗格'); await page.keyboard.press('Escape');
  await page.keyboard.press('Control+Alt+D'); await expect(panes(page)).toHaveCount(8);
  await close(page, 8); await expect(panes(page)).toHaveCount(7); await split(page, 3, '向右分屏');
  await page.keyboard.press('Control+Shift+T'); await expect(page.getByRole('tab')).toHaveCount(2);
  await page.getByRole('button', { name: '分屏', exact: true }).click();
  await expect(page.getByRole('menu')).toContainText('会话数量上限');
  expect(await page.evaluate(() => (window as any).__testEvidence.prepared.length)).toBe(10);
});

test('failed nested creation and restart retain terminal elements, positions and divider state', async ({ page }) => {
  await ready(page, { prepareFailureAt: 4 }); await three(page);
  await retainElements(page);
  const before = await panes(page).evaluateAll(elements => elements.map(el => ({ id: el.id, rect: el.getBoundingClientRect().toJSON() })));
  await page.getByRole('button', { name: '分屏窗格 1', exact: true }).click(); await page.getByRole('menuitem', { name: '向下分屏', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('测试启动失败'); await page.getByRole('button', { name: '关闭提示', exact: true }).click();
  await expectRetained(page, 3);
  await expect.poll(() => panes(page).evaluateAll(elements => elements.map(el => ({ id: el.id, rect: el.getBoundingClientRect().toJSON() })))).toEqual(before);
  const sessionIds = await ids(page); const originalRight = await panes(page).nth(1).boundingBox();
  await page.evaluate(id => (window as any).__finishSession(id), sessionIds[1]);
  await panes(page).nth(1).getByRole('button', { name: '重新启动' }).click();
  await expect(panes(page).nth(1).locator('.finished-bar')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(4);
  expect(await panes(page).nth(1).boundingBox()).toEqual(originalRight);
  expect(await page.evaluate(() => (window as any).__testEvidence.forgotten)).toEqual([sessionIds[1]]);
  expect(await page.evaluate(() => (window as any).__testEvidence.events.includes('view_failed'))).toBe(false);
});

test('font changes update minimum split dimensions and workspace schema 1 saves every nested pane', async ({ page }) => {
  await page.setViewportSize({ width: 1100, height: 900 }); await ready(page); await three(page); await split(page, 1, '向下分屏');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.workspace?.tabs.length)).toBe(4);
  expect(await page.evaluate(() => (window as any).__testEvidence.workspace.schemaVersion)).toBe(1);
  await panes(page).last().locator('textarea').focus();
  for (let i = 0; i < 6; i++) await page.keyboard.press('Control+-');
  await page.getByRole('button', { name: '分屏', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: '向右分屏', exact: true })).toBeEnabled();
  await expect(page.getByRole('menuitem', { name: '向下分屏', exact: true })).toBeEnabled();
  await page.keyboard.press('Escape'); await panes(page).last().locator('textarea').focus(); await page.keyboard.press('Control+0');
  await page.getByRole('button', { name: '分屏', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: '向右分屏', exact: true })).toBeDisabled();
  await expect(page.getByRole('menuitem', { name: '向下分屏', exact: true })).toBeDisabled();
});
