use super::chrome::Palette;
use crate::text::TextBuffer;
use gpui::{prelude::*, *};
use std::ops::Range;

pub enum InputEvent {
    Submit,
    Cancel,
}
pub struct Input {
    pub buffer: TextBuffer,
    pub focus: FocusHandle,
    pub light: bool,
    placeholder: String,
    bounds: Bounds<Pixels>,
    layout: Option<ShapedLine>,
    selecting: bool,
    selection_anchor: usize,
    scroll: Pixels,
}
impl EventEmitter<InputEvent> for Input {}
impl Input {
    pub fn new(text: String, placeholder: &str, light: bool, cx: &mut Context<Self>) -> Self {
        Self {
            buffer: TextBuffer::new(text),
            light,
            focus: cx.focus_handle(),
            placeholder: placeholder.into(),
            bounds: Bounds::default(),
            layout: None,
            selecting: false,
            selection_anchor: 0,
            scroll: px(0.0),
        }
    }
    pub fn text(&self) -> &str {
        &self.buffer.text
    }
    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let mut handled = true;
        match key.key.as_str() {
            "enter" => cx.emit(InputEvent::Submit),
            "escape" => cx.emit(InputEvent::Cancel),
            "backspace" => self.buffer.delete(false),
            "delete" => self.buffer.delete(true),
            "left" => self.buffer.move_cursor(false, key.modifiers.shift),
            "right" => self.buffer.move_cursor(true, key.modifiers.shift),
            "home" => self.buffer.selection = 0..0,
            "end" => {
                let end = self.buffer.text.len();
                self.buffer.selection = end..end;
            }
            "a" if key.modifiers.control => self.buffer.selection = 0..self.buffer.text.len(),
            "c" | "x" if key.modifiers.control => {
                let text = &self.buffer.text[self.buffer.selection.clone()];
                if !text.is_empty() {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.into()));
                }
                if key.key == "x" {
                    self.buffer.replace(None, "", 4096);
                }
            }
            "v" if key.modifiers.control => {
                if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.replace_text_in_range(None, &text, window, cx);
                }
            }
            _ => handled = false,
        }
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }
    fn point_index(&self, position: Point<Pixels>) -> usize {
        self.layout
            .as_ref()
            .map(|line| line.closest_index_for_x(position.x - self.bounds.left() + self.scroll))
            .unwrap_or(0)
            .min(self.buffer.text.len())
    }
}
impl Render for Input {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::new(self.light);
        let prepaint = cx.entity();
        let paint = cx.entity();
        let focus = self.focus.clone();
        div()
            .id("text-input")
            .key_context("TextInput")
            .track_focus(&self.focus)
            .flex_1()
            .min_w_0()
            .h(px(34.0))
            .px_2()
            .bg(rgb(p.elevated))
            .border_1()
            .border_color(rgb(p.line))
            .rounded_md()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::key))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.focus.focus(window);
                    let index = this.point_index(event.position);
                    this.buffer.selection = index..index;
                    this.buffer.selection_reversed = false;
                    this.selection_anchor = index;
                    this.selecting = true;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                this.selecting &= event.pressed_button == Some(MouseButton::Left);
                if this.selecting {
                    let index = this.point_index(event.position);
                    let start = this.selection_anchor;
                    this.buffer.selection = start.min(index)..start.max(index);
                    this.buffer.selection_reversed = index < start;
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.selecting = false),
            )
            .child(
                canvas(
                    move |bounds, window, cx| {
                        prepaint.update(cx, |this, _| {
                            this.bounds = bounds;
                            let empty = this.buffer.text.is_empty();
                            let text = if empty {
                                this.placeholder.clone()
                            } else {
                                this.buffer.text.clone()
                            };
                            let run = TextRun {
                                len: text.len(),
                                font: font("Segoe UI"),
                                color: rgb(if empty { p.muted } else { p.text }).into(),
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let line = window.text_system().shape_line(
                                text.into(),
                                px(14.0),
                                &[run],
                                None,
                            );
                            let caret = line.x_for_index(this.buffer.cursor());
                            if caret - this.scroll > bounds.size.width - px(4.0) {
                                this.scroll = caret - bounds.size.width + px(4.0);
                            }
                            if caret < this.scroll {
                                this.scroll = caret;
                            }
                            let origin = point(bounds.left() - this.scroll, bounds.top() + px(6.0));
                            let selection = (!this.buffer.selection.is_empty()).then(|| {
                                fill(
                                    Bounds::new(
                                        point(
                                            origin.x
                                                + line.x_for_index(this.buffer.selection.start),
                                            origin.y,
                                        ),
                                        size(
                                            line.x_for_index(this.buffer.selection.end)
                                                - line.x_for_index(this.buffer.selection.start),
                                            px(20.0),
                                        ),
                                    ),
                                    rgb(p.selected),
                                )
                            });
                            let cursor = this.focus.is_focused(window).then(|| {
                                fill(
                                    Bounds::new(
                                        point(origin.x + caret, origin.y),
                                        size(px(1.5), px(20.0)),
                                    ),
                                    rgb(p.accent),
                                )
                            });
                            this.layout = Some(line.clone());
                            (line, origin, selection, cursor)
                        })
                    },
                    move |bounds, (line, origin, selection, cursor), window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, paint), cx);
                        if let Some(selection) = selection {
                            window.paint_quad(selection);
                        }
                        if let Err(error) = line.paint(origin, px(20.0), window, cx) {
                            tracing::warn!(%error,"input paint failed");
                        }
                        if let Some(cursor) = cursor {
                            window.paint_quad(cursor);
                        }
                    },
                )
                .size_full(),
            )
    }
}
impl EntityInputHandler for Input {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        actual: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.buffer.byte_range(range);
        *actual = Some(self.buffer.utf16_range(r.clone()));
        Some(self.buffer.text[r].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.buffer.utf16_range(self.buffer.selection.clone()),
            reversed: self.buffer.selection_reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.buffer
            .marked
            .clone()
            .map(|r| self.buffer.utf16_range(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text: String = text.chars().filter(|c| !c.is_control()).collect();
        self.buffer.replace(range, &text, 4096);
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
        self.buffer.compose(range, text, selected, 4096);
        cx.notify();
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let byte = self.buffer.byte_offset(range.start);
        let x = self
            .layout
            .as_ref()
            .map_or(px(0.0), |line| line.x_for_index(byte));
        Some(Bounds::new(
            point(self.bounds.left() + x - self.scroll, self.bounds.top()),
            size(px(2.0), self.bounds.size.height),
        ))
    }
    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.buffer.utf16_offset(self.point_index(point)))
    }
}
