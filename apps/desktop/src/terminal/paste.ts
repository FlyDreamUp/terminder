import { MAX_PASTE } from '../contracts/models';
export function inspectPaste(text: string, maxBytes: number, confirmMultiline: boolean): { bytes: number; confirm: boolean; reason: string } {
  const bytes = new TextEncoder().encode(text).length;
  if (bytes > Math.min(MAX_PASTE, maxBytes)) throw new Error(`粘贴超过 ${Math.min(MAX_PASTE, maxBytes).toLocaleString()} 字节，未发送任何内容`);
  // C0/DEL controls other than CR/LF/TAB can alter the application rather than insert text.
  const controls = [...text].some(c => { const n = c.charCodeAt(0); return (n < 32 && ![9, 10, 13].includes(n)) || n === 127; });
  const multiline = /[\r\n]/.test(text);
  return { bytes, confirm: controls || (multiline && confirmMultiline), reason: controls ? '文本含终端控制字符，可能触发快捷操作或执行命令。' : '文本含多行内容，前台程序可能立即执行其中的命令。' };
}
