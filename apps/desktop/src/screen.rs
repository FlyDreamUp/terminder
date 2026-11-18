use alacritty_terminal::{
    Term,
    event::{Event, EventListener, WindowSize},
    grid::Dimensions,
    term::{Config, Osc52, TermMode, cell::Flags},
    vte::ansi::{Color, Processor, Rgb},
};
use std::{cell::RefCell, rc::Rc};
use terminder_session_core::model::TerminalSize;

#[derive(Clone, Default)]
pub struct Events(Rc<RefCell<(Vec<Event>, bool)>>);
impl EventListener for Events {
    fn send_event(&self, event: Event) {
        if matches!(
            event,
            Event::PtyWrite(_)
                | Event::Title(_)
                | Event::ColorRequest(..)
                | Event::TextAreaSizeRequest(_)
        ) {
            let mut pending = self.0.borrow_mut();
            if pending.0.len() < 256 {
                pending.0.push(event);
            } else {
                pending.1 = true;
            }
        }
    }
}

struct Size(TerminalSize);
impl Dimensions for Size {
    fn columns(&self) -> usize {
        self.0.cols as usize
    }
    fn screen_lines(&self) -> usize {
        self.0.rows as usize
    }
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }
}

pub struct Screen {
    pub term: Term<Events>,
    parser: Processor,
    events: Events,
    pub title: String,
    pub size: TerminalSize,
    pub cell_width: u16,
    pub cell_height: u16,
    pub light: bool,
}
impl Screen {
    pub fn new(size: TerminalSize) -> Self {
        Self::configured(size, 5000, false)
    }
    pub fn configured(size: TerminalSize, history: usize, light: bool) -> Self {
        let events = Events::default();
        let config = Config {
            scrolling_history: history,
            osc52: Osc52::Disabled,
            kitty_keyboard: false,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &Size(size), events.clone()),
            parser: Processor::new(),
            events,
            title: String::new(),
            size,
            cell_width: 9,
            cell_height: 21,
            light,
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> anyhow::Result<Vec<Vec<u8>>> {
        anyhow::ensure!(bytes.len() <= 32768, "output block exceeds 32 KiB");
        self.parser.advance(&mut self.term, bytes);
        let (events, overflow) = std::mem::take(&mut *self.events.0.borrow_mut());
        anyhow::ensure!(!overflow, "terminal reply budget exceeded");
        let mut replies = Vec::new();
        for event in events {
            match event {
                Event::PtyWrite(text) => replies.push(text.into_bytes()),
                Event::Title(title) => {
                    self.title = title.chars().filter(|c| !c.is_control()).take(80).collect()
                }
                Event::ColorRequest(index, format) => {
                    let color = self.term.colors()[index]
                        .map(|v| (u32::from(v.r) << 16) | (u32::from(v.g) << 8) | u32::from(v.b))
                        .unwrap_or_else(|| self.default_color(index));
                    replies.push(
                        format(Rgb {
                            r: (color >> 16) as u8,
                            g: (color >> 8) as u8,
                            b: color as u8,
                        })
                        .into_bytes(),
                    );
                }
                Event::TextAreaSizeRequest(format) => replies.push(
                    format(WindowSize {
                        num_lines: self.size.rows,
                        num_cols: self.size.cols,
                        cell_width: self.cell_width,
                        cell_height: self.cell_height,
                    })
                    .into_bytes(),
                ),
                _ => {}
            }
        }
        Ok(replies)
    }
    pub fn configure(&mut self, history: usize, light: bool) {
        self.light = light;
        self.term.set_options(Config {
            scrolling_history: history,
            osc52: Osc52::Disabled,
            kitty_keyboard: false,
            ..Config::default()
        });
    }
    fn default_color(&self, index: usize) -> u32 {
        if self.light {
            match index {
                256 | 267 => 0x1f2937,
                258 => 0x0369a1,
                268 => 0x64748b,
                257 => 0xf8fafc,
                _ => palette(index),
            }
        } else {
            palette(index)
        }
    }
    pub fn resolve_color(&self, color: Color) -> u32 {
        let index = match color {
            Color::Spec(value) => {
                return (u32::from(value.r) << 16) | (u32::from(value.g) << 8) | u32::from(value.b);
            }
            Color::Indexed(value) => value as usize,
            Color::Named(value) => value as usize,
        };
        self.term.colors()[index]
            .map(|v| (u32::from(v.r) << 16) | (u32::from(v.g) << 8) | u32::from(v.b))
            .unwrap_or_else(|| self.default_color(index))
    }
    pub fn find(&mut self, query: &str, backwards: bool) -> anyhow::Result<bool> {
        use alacritty_terminal::{
            index::{Boundary, Column, Direction, Line, Point, Side},
            selection::{Selection, SelectionType},
            term::search::RegexSearch,
        };
        anyhow::ensure!(query.len() <= 1024, "搜索表达式超过 1024 字节");
        if query.is_empty() {
            self.term.selection = None;
            return Ok(false);
        }
        let mut regex =
            RegexSearch::new(query).map_err(|e| anyhow::anyhow!("搜索表达式无效：{e}"))?;
        let origin = self
            .term
            .selection
            .as_ref()
            .and_then(|s| s.to_range(&self.term))
            .map(|s| {
                if backwards {
                    s.start.sub(&self.term, Boundary::Grid, 1)
                } else {
                    s.end.add(&self.term, Boundary::Grid, 1)
                }
            })
            .unwrap_or(Point::new(
                Line(-(self.term.grid().display_offset() as i32)),
                Column(0),
            ));
        let direction = if backwards {
            Direction::Left
        } else {
            Direction::Right
        };
        if let Some(found) = self.term.search_next(
            &mut regex,
            origin,
            direction,
            if backwards { Side::Left } else { Side::Right },
            None,
        ) {
            let mut selection = Selection::new(SelectionType::Simple, *found.start(), Side::Left);
            selection.update(*found.end(), Side::Right);
            self.term.scroll_to_point(*found.start());
            self.term.selection = Some(selection);
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn resize(&mut self, size: TerminalSize) -> anyhow::Result<()> {
        size.validate()?;
        anyhow::ensure!(size.cols >= 2, "terminal requires at least two columns");
        self.term.resize(Size(size));
        self.size = size;
        Ok(())
    }
    pub fn text(&self) -> String {
        let content = self.term.renderable_content();
        let mut text = String::new();
        let mut previous_line = None;
        for indexed in content.display_iter {
            if previous_line.is_some_and(|line| line != indexed.point.line) {
                text.push('\n');
            }
            previous_line = Some(indexed.point.line);
            if !indexed.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                text.push(indexed.cell.c);
                if let Some(extra) = indexed.cell.zerowidth() {
                    text.extend(extra);
                }
            }
        }
        text
    }
    pub fn paste(&self, text: &str) -> anyhow::Result<Vec<u8>> {
        anyhow::ensure!(text.len() <= 256 * 1024, "paste exceeds 256 KiB");
        // Strip ESC so pasted content cannot terminate the bracketed-paste envelope.
        let text = text
            .replace('\u{1b}', "")
            .replace("\r\n", "\n")
            .replace('\r', "\n");
        Ok(if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.replace('\n', "\r").into_bytes()
        })
    }
}

pub fn palette(index: usize) -> u32 {
    const ANSI: [u32; 16] = [
        0x10151c, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xcbd5e1, 0x64748b,
        0xff8b94, 0xb5e890, 0xffd58b, 0x8ac6ff, 0xe0a8ff, 0x80d5df, 0xf1f5f9,
    ];
    match index {
        0..=15 => ANSI[index],
        16..=231 => {
            let n = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { 55 + 40 * v };
            ((level(n / 36) << 16) | (level(n / 6 % 6) << 8) | level(n % 6)) as u32
        }
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            (v * 0x010101) as u32
        }
        257 => 0x10151c,
        258 => 0x7dd3fc,
        259..=266 => {
            let rgb = palette(index - 259);
            (rgb & 0xfefefe) >> 1
        }
        267 => 0xf1f5f9,
        268 => 0x7c8796,
        _ => 0xdbe4f0,
    }
}
pub fn key_bytes(
    key: &str,
    control: bool,
    alt: bool,
    shift: bool,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if control && key == "space" {
        return Some(if alt { vec![27, 0] } else { vec![0] });
    }
    if control && key.len() == 1 {
        let c = key.as_bytes()[0].to_ascii_lowercase();
        let value = match c {
            b'a'..=b'z' => c - b'a' + 1,
            b' ' | b'@' => 0,
            b'[' => 27,
            b'\\' => 28,
            b']' => 29,
            b'^' => 30,
            b'_' => 31,
            _ => return None,
        };
        return Some(if alt { vec![27, value] } else { vec![value] });
    }
    let modifier = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(control);
    let arrow = match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };
    if let Some(code) = arrow {
        return Some(
            if modifier > 1 {
                format!("\x1b[1;{modifier}{code}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\x1bO{code}")
            } else {
                format!("\x1b[{code}")
            }
            .into_bytes(),
        );
    }
    let value = match key {
        "enter" => "\r".to_string(),
        "backspace" if control => "\x17".into(),
        "backspace" => "\x7f".into(),
        "escape" => "\x1b".into(),
        "tab" if shift => "\x1b[Z".into(),
        "tab" => "\t".into(),
        "insert" | "delete" | "pageup" | "pagedown" => {
            let n = match key {
                "insert" => 2,
                "delete" => 3,
                "pageup" => 5,
                _ => 6,
            };
            if modifier > 1 {
                format!("\x1b[{n};{modifier}~")
            } else {
                format!("\x1b[{n}~")
            }
        }
        "f1" | "f2" | "f3" | "f4" => {
            let code = match key {
                "f1" => 'P',
                "f2" => 'Q',
                "f3" => 'R',
                _ => 'S',
            };
            if modifier > 1 {
                format!("\x1b[1;{modifier}{code}")
            } else {
                format!("\x1bO{code}")
            }
        }
        "f5" | "f6" | "f7" | "f8" | "f9" | "f10" | "f11" | "f12" => {
            let n = match key {
                "f5" => 15,
                "f6" => 17,
                "f7" => 18,
                "f8" => 19,
                "f9" => 20,
                "f10" => 21,
                "f11" => 23,
                _ => 24,
            };
            if modifier > 1 {
                format!("\x1b[{n};{modifier}~")
            } else {
                format!("\x1b[{n}~")
            }
        }
        _ if alt && key.chars().count() == 1 => format!("\x1b{key}"),
        _ => return None,
    };
    Some(value.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen() -> Screen {
        Screen::new(TerminalSize { cols: 80, rows: 24 })
    }
    #[test]
    fn parses_split_utf8_colors_cursor_and_alternate_screen() {
        let mut s = screen();
        let bytes = "\x1b[31mHello 中文\x1b[0m".as_bytes();
        for part in bytes.chunks(2) {
            s.feed(part).unwrap();
        }
        assert!(s.text().contains("Hello 中文"));
        assert_eq!(
            s.term.grid()[alacritty_terminal::index::Point::new(
                alacritty_terminal::index::Line(0),
                alacritty_terminal::index::Column(0)
            )]
            .fg,
            Color::Named(alacritty_terminal::vte::ansi::NamedColor::Red)
        );
        s.feed(b"\x1b[?1049h\x1b[Halt").unwrap();
        assert!(s.text().contains("alt"));
        assert!(!s.text().contains("Hello"));
        s.feed(b"\x1b[?1049l").unwrap();
        assert!(s.text().contains("Hello 中文"));
        let replies = s.feed(b"\x1b[6n").unwrap();
        assert!(replies.iter().any(|b| b.starts_with(b"\x1b[")));
    }
    #[test]
    fn input_modes_and_paste_are_bounded() {
        assert_eq!(
            key_bytes("c", true, false, false, TermMode::NONE),
            Some(vec![3])
        );
        assert_eq!(
            key_bytes("up", false, false, false, TermMode::APP_CURSOR),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_bytes("left", true, false, false, TermMode::NONE),
            Some(b"\x1b[1;5D".to_vec())
        );
        let mut s = screen();
        s.feed(b"\x1b[?2004h").unwrap();
        assert_eq!(s.paste("a\r\nb").unwrap(), b"\x1b[200~a\nb\x1b[201~");
        assert!(s.paste(&"x".repeat(256 * 1024 + 1)).is_err());
        assert!(s.resize(TerminalSize { cols: 0, rows: 24 }).is_err());
        s.resize(TerminalSize {
            cols: 100,
            rows: 30,
        })
        .unwrap();
        assert_eq!(s.term.columns(), 100);
    }
    #[test]
    fn osc52_does_not_read_or_write_clipboard() {
        let mut s = screen();
        assert!(
            s.feed(b"\x1b]52;c;?\x07\x1b]52;c;aGVsbG8=\x07")
                .unwrap()
                .is_empty()
        );
    }
}

#[derive(Clone, Copy)]
pub struct MouseReport {
    pub button: u8,
    pub column: u16,
    pub row: u16,
    pub release: bool,
    pub motion: bool,
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}
pub fn mouse_bytes(mode: TermMode, event: MouseReport) -> Option<Vec<u8>> {
    if !mode.intersects(TermMode::MOUSE_MODE) || event.shift {
        return None;
    }
    if event.motion
        && !(mode.contains(TermMode::MOUSE_MOTION)
            || (mode.contains(TermMode::MOUSE_DRAG) && event.button < 3))
    {
        return None;
    }
    let code = event.button
        + if event.motion { 32 } else { 0 }
        + if event.alt { 8 } else { 0 }
        + if event.control { 16 } else { 0 };
    let (x, y) = (u32::from(event.column) + 1, u32::from(event.row) + 1);
    if mode.contains(TermMode::SGR_MOUSE) {
        return Some(
            format!(
                "\x1b[<{code};{x};{y}{}",
                if event.release { 'm' } else { 'M' }
            )
            .into_bytes(),
        );
    }
    let code = if event.release { 3 } else { code };
    if mode.contains(TermMode::UTF8_MOUSE) {
        if x > 2015 || y > 2015 {
            return None;
        }
        let mut text = String::from("\x1b[M");
        for n in [u32::from(code) + 32, x + 32, y + 32] {
            text.push(char::from_u32(n)?);
        }
        Some(text.into_bytes())
    } else {
        if x > 223 || y > 223 {
            return None;
        }
        Some(vec![
            27,
            b'[',
            b'M',
            code + 32,
            (x + 32) as u8,
            (y + 32) as u8,
        ])
    }
}
#[cfg(test)]
mod native_tests {
    use super::*;
    #[test]
    fn function_keys_mouse_reporting_and_dynamic_colors() {
        assert_eq!(
            key_bytes("f12", true, false, false, TermMode::NONE).unwrap(),
            b"\x1b[24;5~"
        );
        assert_eq!(
            key_bytes("f2", false, false, true, TermMode::NONE).unwrap(),
            b"\x1b[1;2Q"
        );
        let mouse = MouseReport {
            button: 0,
            column: 12,
            row: 4,
            release: false,
            motion: false,
            shift: false,
            alt: false,
            control: false,
        };
        assert_eq!(
            mouse_bytes(TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE, mouse).unwrap(),
            b"\x1b[<0;13;5M"
        );
        assert!(
            mouse_bytes(
                TermMode::MOUSE_REPORT_CLICK,
                MouseReport {
                    shift: true,
                    ..mouse
                }
            )
            .is_none()
        );
        let mut screen = Screen::new(TerminalSize { cols: 80, rows: 24 });
        screen.feed(b"\x1b]4;1;rgb:12/34/56\x07").unwrap();
        assert_eq!(screen.resolve_color(Color::Indexed(1)), 0x123456);
    }
    #[test]
    fn search_reaches_scrollback_and_rejects_bad_patterns() {
        let mut screen = Screen::configured(TerminalSize { cols: 40, rows: 4 }, 100, false);
        screen
            .feed("unique中文\r\n1\r\n2\r\n3\r\n4\r\n5\r\n".as_bytes())
            .unwrap();
        assert!(screen.find("unique中文", false).unwrap());
        assert_eq!(
            screen.term.selection_to_string().as_deref(),
            Some("unique中文")
        );
        assert!(screen.term.grid().display_offset() > 0);
        assert!(screen.find("[", false).is_err());
    }
}
