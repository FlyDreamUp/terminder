//! Shared window chrome: palette, vector icons and labelled controls.
use gpui::{prelude::*, *};

#[derive(Clone, Copy)]
pub(super) struct Palette {
    pub surface: u32,
    pub elevated: u32,
    pub hover: u32,
    pub line: u32,
    pub text: u32,
    pub muted: u32,
    pub accent: u32,
    pub selected: u32,
    pub terminal: u32,
    pub danger: u32,
}
impl Palette {
    pub fn new(light: bool) -> Self {
        if light {
            Self {
                surface: 0xf0f3ef,
                elevated: 0xffffff,
                hover: 0xe3eae4,
                line: 0xd3ded6,
                text: 0x253137,
                muted: 0x63736a,
                accent: 0x247b61,
                selected: 0xdceee4,
                terminal: 0xf8fafc,
                danger: 0xa73745,
            }
        } else {
            Self {
                surface: 0x171c24,
                elevated: 0x1e2530,
                hover: 0x29343c,
                line: 0x303a45,
                text: 0xd6dde6,
                muted: 0x96a5b5,
                accent: 0x8ad7c1,
                selected: 0x233d36,
                terminal: 0x10151c,
                danger: 0xf19a9e,
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Icon {
    Terminal,
    Plus,
    Chevron,
    Close,
    Minimize,
    Maximize,
    Restore,
    SplitRight,
    SplitDown,
    Search,
    Restart,
    Settings,
    Up,
    Down,
    Check,
}
/// Keep icons independent of installed symbol fonts and scale them with the window.
pub(super) fn icon(kind: Icon) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let paths: &[&[(f32, f32)]] = match kind {
                Icon::Terminal => &[&[(3., 4.), (7., 8.), (3., 12.)], &[(8., 12.), (13., 12.)]],
                Icon::Plus => &[&[(8., 3.), (8., 13.)], &[(3., 8.), (13., 8.)]],
                Icon::Chevron => &[&[(4., 6.), (8., 10.), (12., 6.)]],
                Icon::Close => &[&[(4., 4.), (12., 12.)], &[(12., 4.), (4., 12.)]],
                Icon::Minimize => &[&[(3., 8.), (13., 8.)]],
                Icon::Maximize => &[&[(3., 3.), (13., 3.), (13., 13.), (3., 13.), (3., 3.)]],
                Icon::Restore => &[
                    &[(5., 3.), (5., 1.), (15., 1.), (15., 11.), (13., 11.)],
                    &[(2., 5.), (11., 5.), (11., 14.), (2., 14.), (2., 5.)],
                ],
                Icon::SplitRight => &[
                    &[(2., 3.), (14., 3.), (14., 13.), (2., 13.), (2., 3.)],
                    &[(8., 3.), (8., 13.)],
                ],
                Icon::SplitDown => &[
                    &[(2., 3.), (14., 3.), (14., 13.), (2., 13.), (2., 3.)],
                    &[(2., 8.), (14., 8.)],
                ],
                Icon::Search => &[
                    &[
                        (7., 2.),
                        (10., 3.),
                        (12., 6.),
                        (11., 9.),
                        (8., 11.),
                        (5., 10.),
                        (3., 7.),
                        (4., 4.),
                        (7., 2.),
                    ],
                    &[(11., 11.), (14., 14.)],
                ],
                Icon::Restart => &[
                    &[
                        (13., 7.),
                        (11., 3.),
                        (6., 2.),
                        (2., 6.),
                        (2., 11.),
                        (6., 14.),
                        (11., 13.),
                    ],
                    &[(9., 7.), (14., 7.), (14., 2.)],
                ],
                Icon::Settings => &[
                    &[(3., 3.), (3., 13.)],
                    &[(8., 3.), (8., 13.)],
                    &[(13., 3.), (13., 13.)],
                    &[(1., 6.), (5., 6.)],
                    &[(6., 10.), (10., 10.)],
                    &[(11., 5.), (15., 5.)],
                ],
                Icon::Up => &[&[(4., 10.), (8., 6.), (12., 10.)]],
                Icon::Down => &[&[(4., 6.), (8., 10.), (12., 6.)]],
                Icon::Check => &[&[(3., 8.), (6., 11.), (13., 4.)]],
            };
            let mut builder = PathBuilder::stroke(px(1.35));
            for points in paths {
                let mut points = points.iter();
                if let Some(&(x, y)) = points.next() {
                    builder.move_to(bounds.origin + point(px(x), px(y)));
                    for &(x, y) in points {
                        builder.line_to(bounds.origin + point(px(x), px(y)));
                    }
                }
            }
            if let Ok(path) = builder.build() {
                window.paint_path(path, window.text_style().color);
            }
        },
    )
    .size(px(16.0))
    .flex_shrink_0()
}
struct Tooltip {
    label: SharedString,
    palette: Palette,
}
impl Render for Tooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(rgb(p.line))
            .bg(rgb(p.elevated))
            .text_color(rgb(p.text))
            .text_xs()
            .shadow_md()
            .child(self.label.clone())
    }
}
pub(super) fn control(
    id: impl Into<ElementId>,
    kind: Icon,
    label: impl Into<SharedString>,
    p: Palette,
) -> Stateful<Div> {
    let label = label.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(28.0))
        .flex_shrink_0()
        .rounded_md()
        .cursor_pointer()
        .text_color(rgb(p.muted))
        .hover(move |s| {
            s.bg(rgb(p.hover))
                .text_color(rgb(if matches!(kind, Icon::Close) {
                    p.danger
                } else {
                    p.text
                }))
        })
        .tooltip(move |_, cx| {
            cx.new(|_| Tooltip {
                label: label.clone(),
                palette: p,
            })
            .into()
        })
        .child(icon(kind))
}
