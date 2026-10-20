import { expect, it } from 'vitest';
import { inspectPaste } from './paste';
it('rejects by UTF-8 byte count before pasting and previews control characters', () => {
  expect(() => inspectPaste('中'.repeat(100), 256, true)).toThrow();
  expect(inspectPaste('echo hi', 1000, true).confirm).toBe(false);
  expect(inspectPaste('echo hi\n', 1000, true).confirm).toBe(true);
  expect(inspectPaste('\x1b[200~echo hi', 1000, false).confirm).toBe(true);
  expect(inspectPaste('a\tb', 1000, true).confirm).toBe(false);
});
