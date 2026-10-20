import type { ITheme } from '@xterm/xterm';
export const dark: ITheme = {
  background: '#11151b', foreground: '#d6dde6', cursor: '#8ad7c1', cursorAccent: '#11151b', selectionBackground: '#365e6466',
  black: '#252c36', red: '#ef8d8d', green: '#94d3a2', yellow: '#e8c98b', blue: '#86b5e0', magenta: '#c4a3df', cyan: '#7fc6ca', white: '#d6dde6',
  brightBlack: '#778393', brightRed: '#f7aaaa', brightGreen: '#b4e3b4', brightYellow: '#f3dba5', brightBlue: '#a9cef2', brightMagenta: '#dac0ed', brightCyan: '#a2e1e2', brightWhite: '#f6f8fc',
};
export const light: ITheme = { ...dark, background: '#fafbf9', foreground: '#253137', cursor: '#287a66', cursorAccent: '#fafbf9', selectionBackground: '#a1ccbe66', black: '#253137', white: '#e1e8e5', brightBlack: '#64726d', brightWhite: '#ffffff', red: '#a73745', green: '#2e7050', yellow: '#856122', blue: '#3267a0', magenta: '#8952a1', cyan: '#236f80' };
export function isDark(theme: string): boolean { return theme === 'dark' || (theme === 'system' && matchMedia('(prefers-color-scheme: dark)').matches); }
