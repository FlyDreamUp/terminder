use crate::{
    bridge::{Bridge, Message},
    config::model::{ClipboardConfig, TerminalConfig},
    screen::{MouseReport, Screen, key_bytes, mouse_bytes},
    text::TextBuffer,
};
use alacritty_terminal::{
    grid::Scroll,
    index::{Column, Line, Point as GridPoint, Side},
    selection::{Selection, SelectionType},
    term::cell::Flags,
};
use gpui::{prelude::*, *};
use std::{ops::Range, time::Duration};
use terminder_session_core::model::{Phase, SessionEvent, TerminalSize};

actions!(terminder, [SendTab, SendBacktab]);

pub struct Pane {
    bridge: Bridge,
    screen: Screen,
    pub focus: FocusHandle,
    pub phase: Phase,
    status: String,
    sequence: u64,
    consumed: u64,
    pub bounds: Bounds<Pixels>,
    cell_width: Pixels,
    selecting: bool,
    composition: TextBuffer,
    pub settings: TerminalConfig,
    terminal_font: Font,
    clipboard: ClipboardConfig,
    pub light: bool,
    reserved: Vec<Keystroke>,
    _subscriptions: Vec<Subscription>,
    last_mouse: Option<(u16, u16, u8)>,
    scroll_remainder: f32,
    pub error: Option<String>,
    pending_paste: Option<String>,
    #[cfg(feature = "test-support")]
    pub frames: usize,
    _reader: Task<()>,
}
struct Frame {
    lines: Vec<(Point<Pixels>, ShapedLine)>,
    backgrounds: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
}
impl Pane {
    pub fn new(
        bridge: Bridge,
        size: TerminalSize,
        settings: TerminalConfig,
        clipboard: ClipboardConfig,
        light: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let receiver = bridge.receiver();
        let reader = cx.spawn(async move |this, cx| {
            let mut batch = 0;
            while let Ok(message) = receiver.recv().await {
                if this
                    .update(cx, |pane: &mut Self, cx| {
                        let old_title = pane.screen.title.clone();
                        let old_phase = pane.phase.clone();
                        pane.consume(message);
                        if old_title != pane.screen.title || old_phase != pane.phase {
                            cx.emit(PaneEvent::Changed);
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
                batch += 1;
                if batch % 8 == 0 {
                    cx.background_executor()
                        .timer(Duration::from_millis(1))
                        .await;
                }
            }
        });
        let focus = cx.focus_handle();
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| {
                this.focus_report(true);
                cx.emit(PaneEvent::Focused);
                cx.notify();
            }),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.focus_report(false);
                this.pending_paste = None;
                cx.notify();
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if this.focus.is_focused(window) {
                    this.focus_report(window.is_window_active());
                    cx.notify();
                }
            }),
        ];
        Self {
            bridge,
            screen: Screen::configured(size, settings.scrollback_lines as usize, light),
            focus,
            phase: Phase::Preparing,
            status: "正在启动终端…".into(),
            sequence: 0,
            consumed: 0,
            bounds: Bounds::default(),
            cell_width: px(8.4),
            selecting: false,
            composition: TextBuffer::default(),
            terminal_font: resolve_font(&settings.font_family, cx),
            settings,
            clipboard,
            light,
            reserved: Vec::new(),
            _subscriptions: subscriptions,
            last_mouse: None,
            scroll_remainder: 0.0,
            error: None,
            pending_paste: None,
            #[cfg(feature = "test-support")]
            frames: 0,
            _reader: reader,
        }
    }
    fn consume(&mut self, message: Message) {
        let result = (|| -> anyhow::Result<()> {
            match message {
                Message::Output(bytes) => {
                    for reply in self.screen.feed(&bytes)? {
                        self.bridge.input(reply)?;
                    }
                    self.sequence += 1;
                    self.consumed += bytes.len() as u64;
                    // ACK means the terminal parser consumed the bytes, never merely queue delivery.
                    self.bridge.ack(self.sequence, self.consumed)?;
                }
                Message::Event(SessionEvent::State { snapshot }) => {
                    self.phase = snapshot.phase;
                    self.status = if self.phase == Phase::Running {
                        String::new()
                    } else {
                        self.phase_label().into()
                    };
                    if let Some(error) = snapshot.cleanup_error {
                        anyhow::bail!("{}", error);
                    }
                }
                Message::Event(SessionEvent::OutputEnd {
                    sequence,
                    bytes,
                    truncated,
                }) => {
                    anyhow::ensure!(
                        sequence == self.sequence && bytes == self.consumed,
                        "output end did not match consumed prefix"
                    );
                    if truncated {
                        self.status = "Output tail truncated".into();
                    }
                }
                Message::Event(SessionEvent::Error { error }) => anyhow::bail!("{}", error),
                Message::Notice(message) => {
                    self.status = message;
                }
                Message::Error(error) => {
                    self.phase = Phase::Finished;
                    anyhow::bail!("{}", error);
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(error.to_string());
            self.status = error.to_string();
            self.bridge.close();
        }
    }
    fn send(&mut self, bytes: Vec<u8>) {
        if self.phase != Phase::Running {
            return;
        }
        self.pending_paste = None;
        self.screen.term.scroll_display(Scroll::Bottom);
        if let Err(error) = self.bridge.input(bytes) {
            self.status = error.to_string();
        }
    }
    pub fn paste(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if let Some(text) = self.pending_paste.take() {
            match self.screen.paste(&text) {
                Ok(bytes) => {
                    self.status = "已确认粘贴".into();
                    self.send(bytes);
                }
                Err(error) => self.status = error.to_string(),
            }
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.stage_paste(text, cx);
        }
    }
    fn stage_paste(&mut self, text: String, cx: &mut Context<Self>) {
        cx.notify();
        if text.len() > self.clipboard.max_paste_bytes {
            self.status = format!("粘贴超过 {} 字节", self.clipboard.max_paste_bytes);
            return;
        }
        if self.clipboard.confirm_multiline && text.chars().any(|c| c.is_control() && c != '\t') {
            self.pending_paste = Some(text);
            self.status = "粘贴包含多行或控制字符，再按粘贴快捷键确认；Escape 取消".into();
        } else {
            match self.screen.paste(&text) {
                Ok(bytes) => self.send(bytes),
                Err(error) => self.status = error.to_string(),
            }
        }
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        if self
            .reserved
            .iter()
            .any(|reserved| reserved.key == key.key && reserved.modifiers == key.modifiers)
        {
            return;
        }
        // Preserve shell interrupts when there is no text to copy.
        if key.key == "c"
            && key.modifiers
                == (Modifiers {
                    control: true,
                    ..Modifiers::none()
                })
            && self.copy_selection(true, window, cx)
        {
            cx.stop_propagation();
            return;
        }
        if key.key == "escape" && self.pending_paste.take().is_some() {
            self.status = "已取消粘贴".into();
            cx.stop_propagation();
            cx.notify();
            return;
        }
        // Text commits go through the native input handler, preserving the IME path.
        if key.key_char.is_some()
            && !key.modifiers.control
            && !key.modifiers.alt
            && !key.modifiers.platform
        {
            return;
        }
        if let Some(bytes) = key_bytes(
            &key.key,
            key.modifiers.control,
            key.modifiers.alt,
            key.modifiers.shift,
            *self.screen.term.mode(),
        ) {
            self.send(bytes);
            cx.stop_propagation();
            cx.notify();
        }
    }
    fn point(&self, position: Point<Pixels>) -> GridPoint {
        let col = ((position.x - self.bounds.left()) / self.cell_width)
            .floor()
            .max(0.0) as usize;
        let row = ((position.y - self.bounds.top()) / px(self.line_height()))
            .floor()
            .max(0.0) as i32;
        GridPoint::new(
            Line(
                row.min(i32::from(self.screen.size.rows) - 1)
                    - self.screen.term.grid().display_offset() as i32,
            ),
            Column(col.min(usize::from(self.screen.size.cols) - 1)),
        )
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window);
        if self.report_mouse(event.position, 0, false, false, event.modifiers) {
            cx.notify();
            return;
        }
        self.selecting = true;
        self.screen.term.selection = Some(Selection::new(
            if event.click_count >= 3 {
                SelectionType::Lines
            } else if event.click_count == 2 {
                SelectionType::Semantic
            } else {
                SelectionType::Simple
            },
            self.point(event.position),
            Side::Left,
        ));
        cx.notify();
    }
    fn mouse_move(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let mouse_button = match event.pressed_button {
            Some(MouseButton::Left) => Some(0),
            Some(MouseButton::Middle) => Some(1),
            Some(MouseButton::Right) => Some(2),
            _ => None,
        };
        self.selecting &= event.pressed_button == Some(MouseButton::Left);
        if self.report_mouse(
            event.position,
            mouse_button.unwrap_or(3),
            false,
            true,
            event.modifiers,
        ) {
            return;
        }
        if self.selecting && event.pressed_button == Some(MouseButton::Left) {
            let point = self.point(event.position);
            if let Some(selection) = &mut self.screen.term.selection {
                selection.update(point, Side::Right);
            }
            cx.notify();
        }
    }
    fn prepaint(&mut self, bounds: Bounds<Pixels>, window: &mut Window) -> Frame {
        #[cfg(feature = "test-support")]
        {
            self.frames += 1;
        }
        self.bounds = bounds;
        let base_font = self.terminal_font.clone();
        let measure = TextRun {
            len: 1,
            font: base_font.clone(),
            color: rgb(0xdbe4f0).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        self.cell_width = window
            .text_system()
            .shape_line("M".into(), px(self.settings.font_size_px), &[measure], None)
            .width
            .max(px(1.0));
        let terminal_size = TerminalSize {
            cols: ((bounds.size.width / self.cell_width).floor() as u16).clamp(2, 500),
            rows: ((bounds.size.height / px(self.line_height())).floor() as u16).clamp(1, 200),
        };
        if terminal_size != self.screen.size
            && self.phase == Phase::Running
            && let Err(error) = self
                .screen
                .resize(terminal_size)
                .and_then(|()| self.bridge.resize(terminal_size))
        {
            self.status = error.to_string();
        }
        self.screen.cell_width = f32::from(self.cell_width).round().max(1.0) as u16;
        self.screen.cell_height = self.line_height() as u16;
        let content = self.screen.term.renderable_content();
        let offset = content.display_offset as i32;
        let mut frame = Frame {
            lines: Vec::new(),
            backgrounds: Vec::new(),
            cursor: None,
        };
        let mut pending: Option<(i32, usize, String, u32, Flags)> = None;
        let flush = |pending: &mut Option<(i32, usize, String, u32, Flags)>,
                     frame: &mut Frame,
                     window: &mut Window| {
            if let Some((row, col, text, fg, flags)) = pending.take() {
                let mut text_font = base_font.clone();
                if flags.contains(Flags::BOLD) {
                    text_font.weight = FontWeight::BOLD;
                }
                if flags.contains(Flags::ITALIC) {
                    text_font.style = FontStyle::Italic;
                }
                let run = TextRun {
                    len: text.len(),
                    font: text_font,
                    color: rgb(fg).into(),
                    background_color: None,
                    underline: flags.contains(Flags::UNDERLINE).then_some(UnderlineStyle {
                        color: None,
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(
                    text.into(),
                    px(self.settings.font_size_px),
                    &[run],
                    None,
                );
                frame.lines.push((
                    point(
                        bounds.left() + self.cell_width * col as f32,
                        bounds.top() + px(self.line_height()) * row as f32,
                    ),
                    shaped,
                ));
            }
        };
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            if row < 0 || row >= i32::from(terminal_size.rows) {
                continue;
            }
            let cell = indexed.cell;
            let (mut fg, mut bg) = (
                self.screen.resolve_color(cell.fg),
                self.screen.resolve_color(cell.bg),
            );
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if content
                .selection
                .as_ref()
                .is_some_and(|s| s.contains(indexed.point))
            {
                bg = 0x284c6a;
            }
            if bg != self.background() {
                frame.backgrounds.push(fill(
                    Bounds::new(
                        point(
                            bounds.left() + self.cell_width * indexed.point.column.0 as f32,
                            bounds.top() + px(self.line_height()) * row as f32,
                        ),
                        size(self.cell_width, px(self.line_height())),
                    ),
                    rgb(bg),
                ));
            }
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let style =
                cell.flags & (Flags::BOLD | Flags::ITALIC | Flags::UNDERLINE | Flags::HIDDEN);
            // Fallback fonts may use a different advance. Anchor non-ASCII clusters to
            // the terminal grid and start the next run at its own column.
            let separate = !cell.c.is_ascii() || cell.zerowidth().is_some();
            if separate
                || pending
                    .as_ref()
                    .is_some_and(|(r, _, _, f, s)| *r != row || *f != fg || *s != style)
            {
                flush(&mut pending, &mut frame, window);
            }
            let run = pending
                .get_or_insert_with(|| (row, indexed.point.column.0, String::new(), fg, style));
            run.2.push(if style.contains(Flags::HIDDEN) {
                ' '
            } else {
                cell.c
            });
            if let Some(extra) = cell.zerowidth() {
                run.2.extend(extra);
            }
            if separate {
                flush(&mut pending, &mut frame, window);
            }
        }
        flush(&mut pending, &mut frame, window);
        let cursor = content.cursor.point;
        let cursor_row = cursor.line.0 + offset;
        if self.focus.is_focused(window)
            && cursor_row >= 0
            && cursor_row < i32::from(terminal_size.rows)
            && content
                .mode
                .contains(alacritty_terminal::term::TermMode::SHOW_CURSOR)
        {
            use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
            let origin = point(
                bounds.left() + self.cell_width * cursor.column.0 as f32,
                bounds.top() + px(self.line_height()) * cursor_row as f32,
            );
            let cursor_color = self.screen.resolve_color(Color::Named(NamedColor::Cursor));
            let (origin, cursor_size) = match content.cursor.shape {
                CursorShape::Underline => (
                    point(origin.x, origin.y + px(self.line_height() - 2.0)),
                    size(self.cell_width, px(2.0)),
                ),
                CursorShape::Beam => (origin, size(px(2.0), px(self.line_height()))),
                _ => (origin, size(self.cell_width, px(self.line_height()))),
            };
            if content.cursor.shape != CursorShape::Hidden {
                let quad = if content.cursor.shape == CursorShape::HollowBlock {
                    fill(Bounds::new(origin, cursor_size), rgba(0x00000000))
                        .border_widths(px(1.0))
                        .border_color(rgb(cursor_color))
                } else {
                    let alpha = if content.cursor.shape == CursorShape::Block {
                        0x66
                    } else {
                        0xff
                    };
                    fill(
                        Bounds::new(origin, cursor_size),
                        rgba((cursor_color << 8) | alpha),
                    )
                };
                frame.cursor = Some(quad);
            }
        }
        if !self.composition.text.is_empty() {
            let run = TextRun {
                len: self.composition.text.len(),
                font: base_font,
                color: rgb(0xffffff).into(),
                background_color: Some(rgb(0x25445a).into()),
                underline: Some(UnderlineStyle {
                    color: None,
                    thickness: px(1.0),
                    wavy: false,
                }),
                strikethrough: None,
            };
            let line = window.text_system().shape_line(
                self.composition.text.clone().into(),
                px(self.settings.font_size_px),
                &[run],
                None,
            );
            frame.lines.push((
                point(
                    bounds.left() + self.cell_width * cursor.column.0 as f32,
                    bounds.top() + px(self.line_height()) * cursor_row.max(0) as f32,
                ),
                line,
            ));
        }
        frame
    }
}
impl Render for Pane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = super::chrome::Palette::new(self.light);
        let prepaint = cx.entity();
        let paint = cx.entity();
        let focus = self.focus.clone();
        let line_height = px(self.line_height());
        div()
            .id("terminal-pane")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(rgb(self.background()))
            .on_key_down(cx.listener(Self::key))
            .on_action(cx.listener(|this, _: &SendTab, _, cx| {
                this.send(vec![9]);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &SendBacktab, _, cx| {
                this.send(b"\x1b[Z".to_vec());
                cx.notify();
            }))
            .child(
                div()
                    .id("terminal-surface")
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_move(cx.listener(Self::mouse_move))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            this.report_mouse(event.position, 0, true, false, event.modifiers);
                            this.selecting = false;
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.focus.focus(window);
                            if !this.report_mouse(event.position, 2, false, false, event.modifiers)
                                && !this.copy_selection(true, window, cx)
                            {
                                this.paste(cx);
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Right,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            this.report_mouse(event.position, 2, true, false, event.modifiers);
                            cx.notify();
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.report_mouse(event.position, 1, false, false, event.modifiers);
                            cx.notify();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(|this, event: &MouseUpEvent, _, cx| {
                            this.report_mouse(event.position, 1, true, false, event.modifiers);
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        this.scroll_remainder += event.delta.pixel_delta(px(this.line_height())).y
                            / px(this.line_height());
                        let lines = this.scroll_remainder.trunc() as i32;
                        this.scroll_remainder -= lines as f32;
                        if lines != 0 {
                            let mode = *this.screen.term.mode();
                            if mode.intersects(alacritty_terminal::term::TermMode::MOUSE_MODE)
                                && !event.modifiers.shift
                            {
                                for _ in 0..lines.unsigned_abs().min(32) {
                                    this.report_mouse(
                                        event.position,
                                        if lines > 0 { 64 } else { 65 },
                                        false,
                                        false,
                                        event.modifiers,
                                    );
                                }
                            } else if mode.contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
                                && !event.modifiers.shift
                            {
                                for _ in 0..lines.unsigned_abs().min(32) {
                                    if let Some(bytes) = key_bytes(
                                        if lines > 0 { "up" } else { "down" },
                                        false,
                                        false,
                                        false,
                                        mode,
                                    ) {
                                        this.send(bytes);
                                    }
                                }
                            } else {
                                this.screen.term.scroll_display(Scroll::Delta(lines));
                            }
                            cx.notify();
                        }
                    }))
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                prepaint.update(cx, |pane, _| pane.prepaint(bounds, window))
                            },
                            move |bounds, frame, window, cx| {
                                window.handle_input(
                                    &focus,
                                    ElementInputHandler::new(bounds, paint),
                                    cx,
                                );
                                for quad in frame.backgrounds {
                                    window.paint_quad(quad);
                                }
                                for (origin, line) in frame.lines {
                                    if let Err(error) = line.paint(origin, line_height, window, cx)
                                    {
                                        tracing::error!(%error, "terminal text paint failed");
                                    }
                                }
                                if let Some(cursor) = frame.cursor {
                                    window.paint_quad(cursor);
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
            .when(self.error.is_some() || !self.status.is_empty(), |s| {
                s.child(
                    div()
                        .px_3()
                        .py_1()
                        .flex_shrink_0()
                        .text_size(px(11.0))
                        .bg(rgb(palette.surface))
                        .text_color(rgb(if self.error.is_some() {
                            palette.danger
                        } else {
                            palette.muted
                        }))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .child(self.error.as_ref().unwrap_or(&self.status).clone()),
                )
            })
    }
}

#[derive(Clone, Copy)]
pub enum PaneEvent {
    Focused,
    Changed,
}
impl EventEmitter<PaneEvent> for Pane {}
impl Pane {
    pub fn title(&self) -> &str {
        &self.screen.title
    }
    pub fn phase_label(&self) -> &'static str {
        match self.phase {
            Phase::Preparing | Phase::Starting => "正在启动",
            Phase::Running => "运行中",
            Phase::Draining | Phase::Closing | Phase::Finalizing => "正在结束",
            Phase::Finished => "已结束",
        }
    }
    pub fn is_running(&self) -> bool {
        self.phase != Phase::Finished
    }
    fn line_height(&self) -> f32 {
        (self.settings.font_size_px * 1.5).ceil()
    }
    fn background(&self) -> u32 {
        if self.light { 0xf8fafc } else { 0x10151c }
    }
    pub fn configure(
        &mut self,
        terminal: TerminalConfig,
        clipboard: ClipboardConfig,
        light: bool,
        reserved: Vec<Keystroke>,
        cx: &mut Context<Self>,
    ) {
        self.screen
            .configure(terminal.scrollback_lines as usize, light);
        self.terminal_font = resolve_font(&terminal.font_family, cx);
        self.settings = terminal;
        self.clipboard = clipboard;
        self.light = light;
        self.reserved = reserved;
        cx.notify();
    }
    pub fn copy(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.copy_selection(false, window, cx);
    }
    fn copy_selection(&mut self, clear: bool, window: &Window, cx: &mut Context<Self>) -> bool {
        let Some(text) = self
            .screen
            .term
            .selection_to_string()
            .filter(|text| !text.is_empty())
        else {
            return false;
        };
        // A copy replaces the clipboard intent; a later paste must not confirm stale text.
        self.pending_paste = None;
        match super::clipboard::write_text(window, &text) {
            Ok(()) => {
                if clear {
                    self.screen.term.selection = None;
                    self.selecting = false;
                }
                self.status = "已复制".into();
            }
            Err(error) => self.status = format!("复制失败：{error:#}"),
        }
        cx.notify();
        // A failed copy still consumes the gesture: never interrupt or paste by accident.
        true
    }
    pub fn find(&mut self, query: &str, backwards: bool, cx: &mut Context<Self>) {
        match self.screen.find(query, backwards) {
            Ok(true) => self.status = "已选中匹配项".into(),
            Ok(false) => self.status = "没有匹配项".into(),
            Err(error) => self.status = error.to_string(),
        }
        cx.notify();
    }
    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        self.screen.term.selection = None;
        cx.notify();
    }
    fn report_mouse(
        &mut self,
        position: Point<Pixels>,
        button: u8,
        release: bool,
        motion: bool,
        modifiers: Modifiers,
    ) -> bool {
        let point = self.point(position);
        let column = point.column.0 as u16;
        let row = (point.line.0 + self.screen.term.grid().display_offset() as i32).max(0) as u16;
        let report = MouseReport {
            button,
            column,
            row,
            release,
            motion,
            shift: modifiers.shift,
            alt: modifiers.alt,
            control: modifiers.control,
        };
        if let Some(bytes) = mouse_bytes(*self.screen.term.mode(), report) {
            if !motion || self.last_mouse != Some((column, row, button)) {
                self.send(bytes);
                self.last_mouse = Some((column, row, button));
            }
            true
        } else {
            false
        }
    }
    fn focus_report(&mut self, focused: bool) {
        if self
            .screen
            .term
            .mode()
            .contains(alacritty_terminal::term::TermMode::FOCUS_IN_OUT)
        {
            self.send(if focused {
                b"\x1b[I".to_vec()
            } else {
                b"\x1b[O".to_vec()
            });
        }
    }
}
impl EntityInputHandler for Pane {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let bytes = self.composition.byte_range(range);
        *actual = Some(self.composition.utf16_range(bytes.clone()));
        Some(self.composition.text[bytes].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self
                .composition
                .utf16_range(self.composition.selection.clone()),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.composition
            .marked
            .clone()
            .map(|r| self.composition.utf16_range(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.composition.take();
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.composition.replace(range, text, 256 * 1024) {
            let bytes = self.composition.take().into_bytes();
            self.send(bytes);
        } else {
            self.status = "文本输入超过容量上限".into();
        }
        cx.notify();
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.composition.compose(range, text, selected, 4096) {
            self.status = "输入法组合文本超过容量上限".into();
        }
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let cursor = self.screen.term.grid().cursor.point;
        Some(Bounds::new(
            point(
                self.bounds.left() + self.cell_width * cursor.column.0 as f32,
                self.bounds.top() + px(self.line_height()) * cursor.line.0.max(0) as f32,
            ),
            size(self.cell_width, px(self.line_height())),
        ))
    }
    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

#[cfg(feature = "test-support")]
impl Pane {
    pub fn test_title(&mut self, title: &str, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.screen.feed(format!("\x1b]2;{title}\x07").as_bytes())?;
        cx.notify();
        Ok(())
    }
    pub fn test_stage_paste(&mut self, text: String, cx: &mut Context<Self>) {
        self.stage_paste(text, cx);
    }
    pub fn test_paste_pending(&self) -> bool {
        self.pending_paste.is_some()
    }
    pub fn test_text(&self) -> String {
        self.screen.text()
    }
    pub fn test_input(&mut self, bytes: Vec<u8>) {
        self.send(bytes);
    }
    pub fn test_status(&self) -> &str {
        &self.status
    }
    pub fn test_selection(&self) -> Option<String> {
        self.screen.term.selection_to_string()
    }
}

fn resolve_font(families: &[String], cx: &App) -> Font {
    let installed = cx.text_system().all_font_names();
    let mut available: Vec<String> = families
        .iter()
        .chain([String::from("Consolas"), String::from("Courier New")].iter())
        .filter_map(|name| {
            installed
                .iter()
                .find(|font| font.eq_ignore_ascii_case(name))
                .cloned()
        })
        .collect();
    available.dedup();
    let mut chosen = font(
        available
            .first()
            .cloned()
            .unwrap_or_else(|| "Consolas".into()),
    );
    if available.len() > 1 {
        chosen.fallbacks = Some(FontFallbacks::from_fonts(
            available.into_iter().skip(1).collect(),
        ));
    }
    chosen
}
