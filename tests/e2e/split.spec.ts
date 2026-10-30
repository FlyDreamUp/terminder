import { test, expect, type Page } from '../../apps/desktop/tests/harness';
import { installHost } from './mock-host';

async function ready(page: Page, options: Parameters<typeof installHost>[1] = {}) {
  await installHost(page, options); await page.goto('/');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBe(1);
}
async function split(page: Page, direction = '向右分屏') {
  await page.getByRole('button', { name: '分屏', exact: true }).click();
  await page.getByRole('menuitem', { name: direction, exact: true }).click();
  await expect(page.locator('.tab-content:not(.inactive) .session-pane')).toHaveCount(2);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(2);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.acknowledgments.length)).toBeGreaterThanOrEqual(2);
}
async function sessionIds(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as any).__testEvidence.prepared.map((p: any) => p.binding.sessionId));
}

for (const direction of ['向右分屏', '向下分屏']) {
  test(`${direction} isolates input, resizes both visible sessions and preserves them when zooming`, async ({ page }) => {
    const errors: string[] = []; page.on('pageerror', error => errors.push(error.message));
    await ready(page); await split(page, direction);
    const panes = page.locator('.session-pane'); const ids = await sessionIds(page);
    const first = (await panes.nth(0).boundingBox())!; const second = (await panes.nth(1).boundingBox())!;
    if (direction === '向右分屏') { expect(first.x + first.width).toBeLessThan(second.x); expect(first.y).toBe(second.y); }
    else { expect(first.y + first.height).toBeLessThan(second.y); expect(first.x).toBe(second.x); }
    expect(await page.evaluate(() => (window as any).__testEvidence.prepared[1].inheritSession)).toBe(ids[0]);
    expect(await page.evaluate(() => (window as any).__testEvidence.prepared[1].profileId)).toBe('pwsh');
    await page.evaluate(() => { (window as any).__testEvidence.inputBySession = []; });
    const labelBefore = (await panes.nth(0).locator('.pane-name').boundingBox())!;
    await panes.nth(0).locator('textarea').focus(); await page.keyboard.type('left');
    const labelAfter = (await panes.nth(0).locator('.pane-name').boundingBox())!;
    expect(labelAfter.x).toBe(labelBefore.x); expect(labelAfter.width).toBe(labelBefore.width);
    await panes.nth(1).locator('textarea').focus(); await page.keyboard.type('right');
    await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.inputBySession.map((x: any) => String.fromCharCode(...x.bytes)).join(''))).toBe('leftright');
    const inputs = await page.evaluate(() => (window as any).__testEvidence.inputBySession as { id: string; bytes: number[] }[]);
    expect(inputs.filter(x => x.id === ids[0]).flatMap(x => x.bytes)).toEqual([...new TextEncoder().encode('left')]);
    expect(inputs.filter(x => x.id === ids[1]).flatMap(x => x.bytes)).toEqual([...new TextEncoder().encode('right')]);
    await page.evaluate(() => { (window as any).__testEvidence.resize = []; });
    const divider = page.getByRole('separator'); const box = (await divider.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2); await page.mouse.down();
    await page.mouse.move(box.x + box.width / 2 + (direction === '向右分屏' ? 100 : 0), box.y + box.height / 2 + (direction === '向下分屏' ? 80 : 0)); await page.mouse.up();
    await expect.poll(() => page.evaluate(() => [...new Set((window as any).__testEvidence.resize.map((x: any) => x.binding.sessionId))].length)).toBe(2);
    const ratio = Number(await divider.getAttribute('aria-valuenow')); expect(ratio).toBeGreaterThan(50);
    await divider.focus(); await page.keyboard.press(direction === '向右分屏' ? 'ArrowLeft' : 'ArrowUp');
    expect(Number(await divider.getAttribute('aria-valuenow'))).toBeLessThan(ratio);
    await page.keyboard.press('Enter'); await expect(divider).toHaveAttribute('aria-valuenow', '50');
    await page.keyboard.press(direction === '向右分屏' ? 'ArrowRight' : 'ArrowDown');
    const savedRatio = await divider.getAttribute('aria-valuenow');
    await page.getByRole('button', { name: '最大化窗格 1', exact: true }).click();
    await expect(panes.nth(1)).toBeHidden(); await expect(panes.nth(0)).toBeVisible();
    await expect(page.getByRole('separator')).toHaveCount(0);
    await page.getByRole('button', { name: '还原分屏', exact: true }).click();
    await expect(panes.nth(1)).toBeVisible(); await expect(divider).toHaveAttribute('aria-valuenow', savedRatio!);
    await page.keyboard.press('Control+Shift+T'); await expect(page.getByRole('tab')).toHaveCount(2);
    await page.getByRole('tab').first().click(); await expect(panes.nth(0)).toBeVisible(); await expect(panes.nth(1)).toBeVisible();
    expect((await sessionIds(page)).slice(0, 2)).toEqual(ids);
    expect(await page.evaluate(() => (window as any).__testEvidence.events.filter((s: string) => s === 'view_failed'))).toEqual([]);
    await page.getByRole('button', { name: '分屏', exact: true }).click();
    await expect(page.getByRole('menuitem', { name: direction })).toBeDisabled();
    await expect(page.getByRole('menuitem', { name: direction === '向右分屏' ? '向下分屏' : '向右分屏' })).toBeEnabled();
    await page.keyboard.press('Escape'); await expect(page.getByRole('button', { name: '分屏', exact: true })).toBeFocused();
    await page.screenshot({ path: `test-results/split-${direction === '向右分屏' ? 'columns' : 'rows'}.png` });
    expect(errors).toEqual([]);
  });
}

test('focus routes search and paste to the selected pane; delayed paste is cancelled after changing pane', async ({ page }) => {
  await ready(page, { clipboardDelayMs: 500 }); await split(page);
  const ids = await sessionIds(page); const panes = page.locator('.session-pane');
  await page.evaluate(ids => { (window as any).__emitOutput(ids[0], '\r\nleft-only-marker\r\n'); (window as any).__emitOutput(ids[1], '\r\nright-only-marker\r\n'); }, ids);
  await panes.first().locator('textarea').focus(); await page.keyboard.press('Control+Shift+F');
  await page.getByRole('textbox', { name: '搜索当前终端' }).fill('left-only-marker'); await expect(page.locator('.search-bar')).toContainText('已定位');
  await panes.nth(1).locator('textarea').focus(); await expect(page.locator('.search-bar')).toHaveCount(0);
  await page.keyboard.press('Control+Shift+F'); await page.getByRole('textbox', { name: '搜索当前终端' }).fill('left-only-marker');
  await expect(page.locator('.search-bar')).toContainText('未找到'); await page.keyboard.press('Escape');
  await page.evaluate(() => { (window as any).__testEvidence.clipboard = 'cancel-paste-after-pane-change'; });
  await page.keyboard.press('Control+Shift+V');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.events.includes('read_clipboard'))).toBe(true);
  await panes.first().locator('textarea').focus();
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.clipboardReads)).toBe(1);
  expect(await page.evaluate(() => (window as any).__testEvidence.inputBySession.some((x: any) => new TextDecoder().decode(Uint8Array.from(x.bytes)).includes('cancel-paste')))).toBe(false);
  await page.evaluate(() => { (window as any).__testEvidence.clipboard = 'paste-to-first'; });
  await page.keyboard.press('Control+Shift+V');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.inputBySession.find((x: any) => new TextDecoder().decode(Uint8Array.from(x.bytes)).includes('paste-to-first'))?.id)).toBe(ids[0]);
});

test('closing one pane waits for output and cleanup; the sibling retains its session and buffer', async ({ page }) => {
  await ready(page, { outputEndDelayMs: 500, repeatFinishedState: true }); await split(page);
  const ids = await sessionIds(page);
  await page.evaluate(id => (window as any).__emitOutput(id, '\r\nkeep-this-buffer\r\n'), ids[0]);
  await page.getByRole('button', { name: '关闭窗格 2', exact: true }).click();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  expect(await page.evaluate(() => (window as any).__testEvidence.closed)).toEqual([]);
  await page.getByRole('button', { name: '关闭窗格 2', exact: true }).click();
  await page.getByRole('button', { name: '关闭窗格', exact: true }).click();
  await expect(page.locator('.session-pane')).toHaveCount(2);
  await expect(page.locator('.session-pane')).toHaveCount(1);
  await expect(page.getByRole('tab')).toHaveCount(1);
  await expect(page.getByRole('listitem').filter({ hasText: 'keep-this-buffer' })).toBeVisible();
  expect(await page.evaluate(() => (window as any).__testEvidence.forgotten)).toEqual([ids[1]]);
  await expect(page.locator('.xterm-helper-textarea')).toBeFocused();
  await page.keyboard.type('still-running');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.inputBySession.at(-1)?.id)).toBe(ids[0]);
});

test('closing a split tab confirms once and removes both sessions while preserving other tabs', async ({ page }) => {
  await ready(page, { repeatFinishedState: true }); await split(page);
  const ids = await sessionIds(page);
  await page.keyboard.press('Control+Shift+T'); await expect(page.getByRole('tab')).toHaveCount(2);
  await page.getByRole('tab').first().click(); await page.keyboard.press('Control+Shift+W');
  await expect(page.getByRole('dialog')).toContainText('2 个窗格');
  await page.getByRole('button', { name: '关闭标签页', exact: true }).click();
  await expect(page.getByRole('tab')).toHaveCount(1); await expect(page.locator('.session-pane')).toHaveCount(1);
  expect((await page.evaluate(() => (window as any).__testEvidence.forgotten)).sort()).toEqual(ids.sort());
});

test('split creation failure and session limits leave the existing terminal intact', async ({ page }) => {
  await ready(page, { prepareFailureAt: 2, maxSessions: 2 });
  const original = (await sessionIds(page))[0];
  await page.getByRole('button', { name: '分屏', exact: true }).click(); await page.getByRole('menuitem', { name: '向右分屏' }).click();
  await expect(page.getByRole('alert')).toContainText('测试启动失败'); await expect(page.locator('.session-pane')).toHaveCount(1);
  await split(page);
  expect((await sessionIds(page))[0]).toBe(original);
  await page.keyboard.press('Control+Shift+T'); await expect(page.getByRole('alert')).toContainText('已达到会话数量上限');
  await expect(page.getByRole('tab')).toHaveCount(1); await expect(page.locator('.session-pane')).toHaveCount(2);
});

test('a failed pane cleanup remains visible and retry closes only that pane', async ({ page }) => {
  await ready(page, { forgetFailures: 1 }); await split(page);
  const ids = await sessionIds(page);
  await page.getByRole('button', { name: '关闭窗格 1', exact: true }).click(); await page.getByRole('button', { name: '关闭窗格', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('宿主繁忙'); await expect(page.locator('.session-pane')).toHaveCount(2);
  await page.getByRole('button', { name: '关闭窗格 1', exact: true }).click(); await expect(page.locator('.session-pane')).toHaveCount(1);
  expect(await page.evaluate(() => (window as any).__testEvidence.closed)).toEqual([ids[0]]);
  expect(await page.evaluate(() => (window as any).__testEvidence.forgotten)).toEqual([ids[0]]);
});

test('partial tab close failure leaves the failed pane available for retry', async ({ page }) => {
  await ready(page, { closeFailureAt: 1 }); await split(page);
  await page.keyboard.press('Control+Shift+W'); await page.getByRole('button', { name: '关闭标签页', exact: true }).click();
  await expect(page.locator('.session-pane')).toHaveCount(1); await expect(page.getByRole('alert')).toContainText('测试关闭失败');
  await page.getByRole('button', { name: '关闭 PowerShell 7', exact: true }).click(); await page.getByRole('button', { name: '关闭终端', exact: true }).click();
  await expect(page.getByRole('tab')).toHaveCount(0);
});

test('restart replaces only the finished pane and permits retry after preparation fails', async ({ page }) => {
  await ready(page, { prepareFailureAt: 3, maxSessions: 2 }); await split(page, '向下分屏');
  const ids = await sessionIds(page);
  await page.evaluate(id => (window as any).__finishSession(id), ids[1]);
  await page.getByRole('button', { name: '重新启动' }).click(); await expect(page.getByRole('alert')).toContainText('测试启动失败');
  await page.keyboard.press('Control+Shift+T');
  await expect(page.getByRole('tab')).toHaveCount(1);
  await expect(page.getByRole('alert').filter({ hasText: '已达到会话数量上限' })).toBeVisible();
  await page.getByRole('button', { name: '重新启动' }).click(); await expect(page.locator('.finished-bar')).toHaveCount(0);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(3);
  await expect(page.locator('.session-pane')).toHaveCount(2); await expect(page.getByRole('separator')).toHaveAttribute('aria-orientation', 'horizontal');
  expect(await page.evaluate(() => (window as any).__testEvidence.forgotten)).toEqual([ids[1]]);
  expect(await page.evaluate(() => (window as any).__testEvidence.closed)).toEqual([]);
});

test('split startup information stays compatible with schema 1 and restores as separate tabs', async ({ page }) => {
  await ready(page); await split(page);
  await page.getByRole('tab').focus(); await page.keyboard.press('F2'); await page.getByRole('textbox', { name: '标签名称' }).fill('工作区'); await page.keyboard.press('Enter');
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.workspace?.tabs.map((t: any) => t.title))).toEqual(['工作区', '工作区']);
  const workspace = await page.evaluate(() => (window as any).__testEvidence.workspace);
  expect(workspace.schemaVersion).toBe(1); expect(workspace.activeTab).toBe(workspace.tabs[1].id);
  await installHost(page, { workspace }); await page.reload();
  await page.getByRole('button', { name: '重建标签', exact: true }).click(); await expect(page.getByRole('tab')).toHaveCount(2);
  await expect.poll(() => page.evaluate(() => (window as any).__testEvidence.starts)).toBe(2);
  await expect(page.getByRole('separator')).toHaveCount(0);
  await expect(page.getByRole('tab').nth(1)).toHaveAttribute('aria-selected', 'true');
});

test('small viewports prevent unusable splits and keyboard menu navigation remains available', async ({ page }) => {
  await ready(page); await page.setViewportSize({ width: 480, height: 350 });
  const trigger = page.getByRole('button', { name: '分屏', exact: true }); await trigger.focus(); await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: '向右分屏' })).toBeDisabled(); await expect(page.getByRole('menuitem', { name: '向下分屏' })).toBeDisabled();
  await page.keyboard.press('Escape'); await expect(trigger).toBeFocused();
  await page.setViewportSize({ width: 900, height: 720 });
  // Let the resize event and ResizeObserver finish before opening a menu that closes on resize.
  await page.evaluate(() => new Promise<void>(resolve => requestAnimationFrame(() => requestAnimationFrame(() => resolve()))));
  await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: '向右分屏' })).toBeFocused(); await page.keyboard.press('ArrowDown');
  await expect(page.getByRole('menuitem', { name: '向下分屏' })).toBeFocused(); await page.keyboard.press('Enter');
  await expect(page.getByRole('separator')).toHaveAttribute('aria-orientation', 'horizontal');
  await page.getByRole('button', { name: '设置', exact: true }).click(); await page.getByRole('combobox', { name: '主题', exact: true }).selectOption('light');
  await page.getByRole('button', { name: '保存设置' }).click(); await expect(page.getByRole('status')).toContainText('已保存'); await page.getByRole('button', { name: '关闭设置' }).click();
  await page.screenshot({ path: 'test-results/split-light-narrow.png' });
});
