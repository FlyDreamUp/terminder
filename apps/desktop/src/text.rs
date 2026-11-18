use std::ops::Range;
/// UTF-8 storage with UTF-16 conversion at the native input boundary.
#[derive(Default, Debug)]
pub struct TextBuffer {
    pub text: String,
    pub selection: Range<usize>,
    pub marked: Option<Range<usize>>,
    pub selection_reversed: bool,
}
impl TextBuffer {
    pub fn new(text: String) -> Self {
        let end = text.len();
        Self {
            text,
            selection: end..end,
            marked: None,
            selection_reversed: false,
        }
    }
    pub fn utf16_offset(&self, byte: usize) -> usize {
        self.text[..byte.min(self.text.len())]
            .encode_utf16()
            .count()
    }
    pub fn byte_offset(&self, utf16: usize) -> usize {
        let mut offset = 0;
        for (byte, ch) in self.text.char_indices() {
            if offset >= utf16 {
                return byte;
            }
            if offset + ch.len_utf16() > utf16 {
                return byte;
            }
            offset += ch.len_utf16();
        }
        self.text.len()
    }
    pub fn utf16_range(&self, range: Range<usize>) -> Range<usize> {
        self.utf16_offset(range.start)..self.utf16_offset(range.end)
    }
    pub fn byte_range(&self, range: Range<usize>) -> Range<usize> {
        let start = self.byte_offset(range.start);
        let end = self.byte_offset(range.end);
        start.min(end)..end.max(start)
    }
    pub fn replace(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        max_bytes: usize,
    ) -> bool {
        let range = range_utf16
            .map(|r| self.byte_range(r))
            .or(self.marked.clone())
            .unwrap_or(self.selection.clone());
        if self.text.len() - range.len() + text.len() > max_bytes {
            return false;
        }
        self.text.replace_range(range.clone(), text);
        let end = range.start + text.len();
        self.selection = end..end;
        self.marked = None;
        self.selection_reversed = false;
        true
    }
    pub fn compose(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        max_bytes: usize,
    ) -> bool {
        let start = range
            .clone()
            .map(|r| self.byte_range(r).start)
            .or(self.marked.as_ref().map(|r| r.start))
            .unwrap_or(self.selection.start);
        if !self.replace(range, text, max_bytes) {
            return false;
        }
        let end = start + text.len();
        self.marked = Some(start..end);
        let relative = TextBuffer::new(text.into());
        let selected = selected
            .map(|r| relative.byte_range(r))
            .unwrap_or(text.len()..text.len());
        self.selection = start + selected.start..start + selected.end;
        true
    }
    pub fn take(&mut self) -> String {
        self.selection = 0..0;
        self.selection_reversed = false;
        self.marked = None;
        std::mem::take(&mut self.text)
    }
    pub fn cursor(&self) -> usize {
        if self.selection_reversed {
            self.selection.start
        } else {
            self.selection.end
        }
    }
    pub fn move_cursor(&mut self, forward: bool, extend: bool) {
        if !extend && !self.selection.is_empty() {
            let position = if forward {
                self.selection.end
            } else {
                self.selection.start
            };
            self.selection = position..position;
            self.selection_reversed = false;
            self.marked = None;
            return;
        }
        let current = self.cursor();
        let anchor = if self.selection_reversed {
            self.selection.end
        } else {
            self.selection.start
        };
        let position = if forward {
            self.text[current..]
                .chars()
                .next()
                .map_or(current, |c| current + c.len_utf8())
        } else {
            self.text[..current]
                .char_indices()
                .last()
                .map_or(0, |(i, _)| i)
        };
        self.selection = if extend {
            anchor.min(position)..anchor.max(position)
        } else {
            position..position
        };
        self.selection_reversed = extend && position < anchor;
        self.marked = None;
    }
    pub fn delete(&mut self, forward: bool) {
        if self.selection.is_empty() {
            let cursor = self.selection.start;
            self.move_cursor(forward, false);
            self.selection = cursor.min(self.selection.start)..cursor.max(self.selection.end);
        }
        self.replace(None, "", usize::MAX);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_surrogates_and_partial_composition_preserve_unicode() {
        let mut text = TextBuffer::new("A🌱中".into());
        assert_eq!(text.utf16_range(1..5), 1..3);
        assert_eq!(text.byte_range(1..3), 1..5);
        assert_eq!(text.byte_offset(2), 1);
        assert!(text.compose(Some(1..3), "你好", Some(1..2), 64));
        assert_eq!(text.text, "A你好中");
        assert_eq!(text.utf16_range(text.selection.clone()), 2..3);
        assert!(text.replace(None, "世界", 64));
        assert_eq!(text.take(), "A世界中");
        assert_eq!(text.selection, 0..0);
    }
    #[test]
    fn shift_selection_extends_and_reverses_across_unicode() {
        let mut text = TextBuffer::new("a中🌱".into());
        text.move_cursor(false, true);
        text.move_cursor(false, true);
        assert_eq!(&text.text[text.selection.clone()], "中🌱");
        text.move_cursor(true, true);
        assert_eq!(&text.text[text.selection.clone()], "🌱");
        text.move_cursor(false, false);
        assert_eq!(text.selection, 4..4);
    }
    #[test]
    fn editing_is_bounded_and_never_splits_utf8() {
        let mut text = TextBuffer::new("a中🌱".into());
        text.delete(false);
        assert_eq!(text.text, "a中");
        assert!(!text.replace(None, "12345", 5));
        assert_eq!(text.text, "a中");
        text.selection = 0..0;
        text.delete(true);
        assert_eq!(text.text, "中");
    }
}
