use super::{
    chrome::{Icon, Palette, control, icon},
    workspace::{CloseTarget, Drag, Modal, ProfileMenu, Workspace},
};
use crate::layout::{Axis, Layout, PaneId};
use gpui::{prelude::*, *};

impl Workspace {
    fn button(&self, id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
        let p = Palette::new(self.light);
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .border_1()
            .border_color(rgb(p.line))
            .bg(rgb(p.surface))
            .text_color(rgb(p.text))
            .hover(move |s| s.bg(rgb(p.hover)))
            .child(label.into())
    }
    fn tab_title(&self, index: usize, cx: &App) -> String {
        let tab = &self.tabs[index];
        if let Some(title) = &tab.title {
            return title.clone();
        }
        self.panes
            .get(&tab.active)
            .map(|entry| {
                let title = entry.view.read(cx).title();
                if title.is_empty() {
                    entry.spec.profile_name.clone()
                } else {
                    title.to_string()
                }
            })
            .unwrap_or_else(|| "终端".into())
    }
    fn title_bar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let caption = |id: &'static str, kind, area| {
            #[cfg(feature = "test-support")]
            let measured = cx.entity();
            let element = div()
                .relative()
                .id(id)
                .w(px(44.0))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(p.muted))
                .window_control_area(area)
                .hover(move |s| {
                    if area == WindowControlArea::Close {
                        s.bg(rgb(0xc42b3b)).text_color(rgb(0xffffff))
                    } else {
                        s.bg(rgb(p.hover)).text_color(rgb(p.text))
                    }
                })
                .child(icon(kind));
            #[cfg(feature = "test-support")]
            let element = element.child(
                canvas(
                    move |bounds, _, cx| {
                        measured.update(cx, |view, _| {
                            view.caption_bounds.insert(id, bounds);
                        });
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
            element
        };
        div()
            .flex()
            .items_center()
            .h(px(42.0))
            .flex_shrink_0()
            .bg(rgb(p.surface))
            .border_b_1()
            .border_color(rgb(p.line))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pl_4()
                    .h_full()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(24.0))
                            .rounded_md()
                            .bg(rgb(p.selected))
                            .text_color(rgb(p.accent))
                            .child(icon(Icon::Terminal)),
                    )
                    .child(
                        div()
                            .text_size(px(15.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Terminder"),
                    ),
            )
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w(px(40.0))
                    .window_control_area(WindowControlArea::Drag),
            )
            .child(
                control("settings", Icon::Settings, "设置 · Ctrl+,", p)
                    .mr_2()
                    .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
            )
            .child(caption(
                "window-minimize",
                Icon::Minimize,
                WindowControlArea::Min,
            ))
            .child(caption(
                "window-maximize",
                if window.is_maximized() {
                    Icon::Restore
                } else {
                    Icon::Maximize
                },
                WindowControlArea::Max,
            ))
            // GPUI forwards these caption hit areas to Windows, including WM_CLOSE.
            // Keep close confirmation and shutdown in on_window_should_close.
            .child(caption(
                "window-close",
                Icon::Close,
                WindowControlArea::Close,
            ))
            .into_any_element()
    }
    fn tab_bar(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let tabs = self
            .tabs
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let active = self.active == index;
                div()
                    .id(("tab", index))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .h_full()
                    .min_w(px(150.0))
                    .max_w(px(240.0))
                    .flex_shrink_0()
                    .cursor_pointer()
                    .border_t_2()
                    .border_color(rgb(if active { p.accent } else { p.surface }))
                    .bg(rgb(if active { p.terminal } else { p.surface }))
                    .text_color(rgb(if active { p.text } else { p.muted }))
                    .hover(move |s| s.bg(rgb(if active { p.terminal } else { p.hover })))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        this.select_tab(index, window, cx);
                        if event.click_count() == 2 {
                            this.rename(window, cx);
                        }
                    }))
                    .child(div().text_color(rgb(p.accent)).child(icon(Icon::Terminal)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(self.tab_title(index, cx)),
                    )
                    .child(
                        control(
                            ("close-tab", index),
                            Icon::Close,
                            "关闭标签 · Ctrl+Shift+W",
                            p,
                        )
                        .size(px(22.0))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.request_close(CloseTarget::Tab(index), window, cx);
                            },
                        )),
                    )
            })
            .collect::<Vec<_>>();
        div()
            .flex()
            .items_center()
            .h(px(42.0))
            .flex_shrink_0()
            .gap_1()
            .pr_3()
            .bg(rgb(p.surface))
            .border_b_1()
            .border_color(rgb(p.line))
            .text_size(px(12.0))
            .child(
                div()
                    .id("tabs")
                    .flex()
                    .h_full()
                    .min_w_0()
                    .overflow_x_scroll()
                    .children(tabs),
            )
            .child(
                div()
                    .relative()
                    .size(px(28.0))
                    .flex_shrink_0()
                    .mx_1()
                    .child(
                        control("new-tab", Icon::Plus, "选择终端并新建标签页", p)
                            .when(self.profile_menu == Some(ProfileMenu::TabBar), |s| {
                                s.bg(rgb(p.selected)).text_color(rgb(p.accent))
                            })
                            .map(|element| {
                                #[cfg(feature = "test-support")]
                                let element =
                                    element.relative().child(Self::profile_probe("new-tab", cx));
                                element
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_profiles(ProfileMenu::TabBar, window, cx)
                            })),
                    )
                    .when(self.profile_menu == Some(ProfileMenu::TabBar), |s| {
                        s.child(self.profile_popup(ProfileMenu::TabBar, window, cx))
                    }),
            )
            .child(div().flex_1())
            .into_any_element()
    }
    fn pane_header(&self, id: PaneId, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let active = self.active_id() == Some(id);
        let split = self.tabs[self.active].layout.ids().len() > 1;
        let entry = &self.panes[&id];
        let pane = entry.view.read(cx);
        let title = if pane.title().is_empty() {
            entry.spec.profile_name.clone()
        } else {
            pane.title().into()
        };
        let running = pane.is_running();
        let actions = [
            (
                "split-right",
                Icon::SplitRight,
                "向右分屏 · Ctrl+Alt+R",
                "split_right",
            ),
            (
                "split-down",
                Icon::SplitDown,
                "向下分屏 · Ctrl+Alt+D",
                "split_down",
            ),
            ("find", Icon::Search, "搜索 · Ctrl+Shift+F", "find"),
        ];
        div()
            .id(SharedString::from(format!("pane-header-{id}")))
            .flex()
            .items_center()
            .h(px(34.0))
            .flex_shrink_0()
            .px_1()
            .gap_1()
            .bg(rgb(if active && split {
                p.selected
            } else {
                p.surface
            }))
            .border_b_1()
            .border_color(rgb(p.line))
            .text_size(px(11.0))
            .on_click(cx.listener(move |this, _, window, cx| this.select_pane(id, window, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .flex_1()
                    .min_w_0()
                    .px_2()
                    .text_color(rgb(if active { p.accent } else { p.muted }))
                    .child(icon(Icon::Terminal))
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(title),
                    ),
            )
            .children(actions.into_iter().map(|(key, glyph, label, action)| {
                control(SharedString::from(format!("{key}-{id}")), glyph, label, p)
                    .size(px(26.0))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.pane_command(id, action, window, cx);
                    }))
            }))
            .when(!running, |s| {
                s.child(
                    control(
                        SharedString::from(format!("restart-{id}")),
                        Icon::Restart,
                        "重新启动 · Ctrl+Shift+Enter",
                        p,
                    )
                    .size(px(26.0))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.pane_command(id, "restart", window, cx);
                    })),
                )
            })
            .child(
                control(
                    SharedString::from(format!("close-pane-{id}")),
                    Icon::Close,
                    "关闭此窗格 · Ctrl+Alt+W",
                    p,
                )
                .size(px(26.0))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.request_close(CloseTarget::Pane(id), window, cx);
                })),
            )
            .into_any_element()
    }
    fn render_layout(&self, layout: Layout, path: Vec<bool>, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        match layout {
            Layout::Pane { id } => {
                let active = self.active_id() == Some(id);
                let split = self.tabs[self.active].layout.ids().len() > 1;
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .border_2()
                    .border_color(rgb(if active && split { p.accent } else { p.line }))
                    .child(self.pane_header(id, cx))
                    .child(self.panes[&id].view.clone())
                    .into_any_element()
            }
            Layout::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let mut left_path = path.clone();
                left_path.push(false);
                let mut right_path = path.clone();
                right_path.push(true);
                let first = self.render_layout(*first, left_path, cx);
                let second = self.render_layout(*second, right_path, cx);
                let measured = cx.entity();
                let measure_path = path.clone();
                let divider_path = path.clone();
                let id: SharedString = format!("divider-{:?}", path).into();
                let divider = div()
                    .id(id)
                    .flex_shrink_0()
                    .bg(rgb(p.surface))
                    .hover(move |s| s.bg(rgb(p.accent)))
                    .when(axis == Axis::Horizontal, |s| {
                        s.w(px(5.0)).h_full().cursor_col_resize()
                    })
                    .when(axis == Axis::Vertical, |s| {
                        s.h(px(5.0)).w_full().cursor_row_resize()
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if let Some(bounds) = this.split_bounds.get(&divider_path).copied() {
                                this.dragging = Some(Drag {
                                    path: divider_path.clone(),
                                    axis,
                                    bounds,
                                });
                                this.focus.focus(window);
                                cx.stop_propagation();
                            }
                        }),
                    );
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .overflow_hidden()
                    .when(axis == Axis::Vertical, |s| s.flex_col())
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                measured.update(cx, |this, _| {
                                    this.split_bounds.insert(measure_path, bounds);
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .min_h_0()
                            .flex_shrink_0()
                            .when(axis == Axis::Horizontal, |s| s.w(relative(ratio)))
                            .when(axis == Axis::Vertical, |s| s.h(relative(ratio)))
                            .child(first),
                    )
                    .child(divider)
                    .child(div().flex().flex_1().min_w_0().min_h_0().child(second))
                    .into_any_element()
            }
        }
    }
    fn profile_popup(
        &self,
        origin: ProfileMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let height = match origin {
            ProfileMenu::TabBar => px(28.0),
            ProfileMenu::Welcome => px(38.0),
        };
        // The zero-size origin is the trigger's top-left in this frame. Deferred
        // painting keeps the shared menu above the outside-click dismiss layer.
        let popup = anchored().offset(point(px(0.0), height + px(8.0)));
        let popup = match origin {
            ProfileMenu::TabBar => popup.snap_to_window_with_margin(px(8.0)),
            ProfileMenu::Welcome => popup,
        };
        div().absolute().top_0().left_0().size_0().child(deferred(
            popup.child(
                div()
                    .relative()
                    .child(self.profile_list(window, cx))
                    .map(|element| {
                        #[cfg(feature = "test-support")]
                        let element = element.child(Self::profile_probe("menu", cx));
                        element
                    }),
            ),
        ))
    }
    fn profile_list(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let current = self
            .active_id()
            .and_then(|id| self.panes.get(&id))
            .map(|e| e.spec.profile_id.as_str());
        let rows = self
            .document
            .config
            .profiles
            .iter()
            .enumerate()
            .map(|(index, profile)| {
                let id = profile.id.clone();
                let selected = self.profile_selection == index;
                div()
                    .id(SharedString::from(format!("profile-{id}")))
                    .map(|element| {
                        #[cfg(feature = "test-support")]
                        let element = element
                            .relative()
                            .child(Self::profile_probe(format!("profile-{id}"), cx));
                        element
                    })
                    .flex()
                    .items_center()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(rgb(if selected { p.selected } else { p.elevated }))
                    .hover(move |s| s.bg(rgb(p.hover)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.new_tab(Some(id.clone()), window, cx)
                    }))
                    .child(div().text_color(rgb(p.accent)).child(icon(Icon::Terminal)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(profile.name.clone()),
                    )
                    .when(self.document.config.default_profile == profile.id, |s| {
                        s.child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(p.muted))
                                .child("默认"),
                        )
                    })
                    .when(current == Some(profile.id.as_str()), |s| {
                        s.child(div().text_color(rgb(p.accent)).child(icon(Icon::Check)))
                    })
            })
            .collect::<Vec<_>>();
        // An overlay keeps the terminal grid stable while opening and dismissing the menu.
        div()
            .id("profile-menu")
            .w(px(280.0))
            .max_h(if self.profile_menu == Some(ProfileMenu::Welcome) {
                window.viewport_size().height / 2.0 - px(16.0)
            } else {
                window.viewport_size().height - px(100.0)
            })
            .text_left()
            .flex()
            .flex_col()
            .p_2()
            .rounded_lg()
            .border_1()
            .border_color(rgb(p.line))
            .bg(rgb(p.elevated))
            .text_color(rgb(p.text))
            .text_size(px(12.0))
            .shadow_lg()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_size(px(10.0))
                    .text_color(rgb(p.muted))
                    .child("新建终端"),
            )
            .child(
                div()
                    .id("profile-options")
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.profile_scroll)
                    .children(rows)
                    .when(self.document.config.profiles.is_empty(), |s| {
                        s.child(div().p_2().child("尚未配置终端，请先打开设置。"))
                    }),
            )
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .text_size(px(10.0))
                    .text_color(rgb(p.muted))
                    .child("↑ ↓ 选择 · Enter 创建 · Esc 关闭"),
            )
            .into_any_element()
    }
    #[cfg(feature = "test-support")]
    fn profile_probe(id: impl Into<String>, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let id = id.into();
        canvas(
            move |bounds, _, cx| {
                view.update(cx, |view, _| {
                    view.profile_bounds.insert(id.clone(), bounds);
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .size_full()
    }
    fn search_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let input = self.search.as_ref().expect("search visible").clone();
        div()
            .absolute()
            .top(px(88.0))
            .right(px(16.0))
            .w(px(430.0))
            .max_w_full()
            .flex()
            .gap_1()
            .items_center()
            .p_1()
            .rounded_lg()
            .border_1()
            .border_color(rgb(p.line))
            .bg(rgb(p.elevated))
            .shadow_lg()
            .occlude()
            .child(input)
            .child(
                control("find-prev", Icon::Up, "上一个匹配", p)
                    .on_click(cx.listener(|this, _, _, cx| this.find(true, cx))),
            )
            .child(
                control("find-next", Icon::Down, "下一个匹配", p)
                    .on_click(cx.listener(|this, _, _, cx| this.find(false, cx))),
            )
            .child(
                control("find-close", Icon::Close, "关闭搜索 · Escape", p)
                    .on_click(cx.listener(|this, _, window, cx| this.close_search(window, cx))),
            )
            .into_any_element()
    }
    #[cfg(feature = "test-support")]
    fn welcome_probe(id: &'static str, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        canvas(
            move |bounds, _, cx| {
                view.update(cx, |view, _| {
                    view.welcome_bounds.insert(id, bounds);
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .size_full()
    }
    fn welcome(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let foreground = if self.light { 0xffffff } else { p.terminal };
        div()
            .id("welcome")
            .size_full()
            .map(|element| {
                #[cfg(feature = "test-support")]
                let element = element.relative().child(Self::welcome_probe("surface", cx));
                element
            })
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .overflow_y_scroll()
            .px_6()
            .py_4()
            .text_center()
            .bg(rgb(p.terminal))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .flex_shrink_0()
                    .max_w_full()
                    .map(|element| {
                        #[cfg(feature = "test-support")]
                        let element = element.relative().child(Self::welcome_probe("body", cx));
                        element
                    })
                    .child(
                        div()
                            .text_size(px(42.0))
                            .line_height(px(48.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(p.accent))
                            .child("❯_"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(10.0))
                            .text_color(rgb(p.muted))
                            .child("YOUR NEXT COMMAND STARTS HERE"),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(24.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("留一处空间，专注眼前。"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_color(rgb(p.muted))
                            .child("打开一个终端，继续你的工作。"),
                    )
                    .child(
                        div()
                            .relative()
                            .mt_5()
                            .child(
                                div()
                                    .id("welcome-new")
                                    .map(|element| {
                                        #[cfg(feature = "test-support")]
                                        let element = element
                                            .relative()
                                            .child(Self::welcome_probe("new", cx));
                                        element
                                    })
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .h(px(38.0))
                                    .px_4()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .bg(rgb(p.accent))
                                    .text_color(rgb(foreground))
                                    .hover(|s| s.opacity(0.9))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_profiles(ProfileMenu::Welcome, window, cx)
                                    }))
                                    .child(icon(Icon::Plus))
                                    .child("新建终端")
                                    .child(div().ml_2().child(icon(Icon::Chevron))),
                            )
                            .when(self.profile_menu == Some(ProfileMenu::Welcome), |s| {
                                s.child(self.profile_popup(ProfileMenu::Welcome, window, cx))
                            }),
                    )
                    .child(
                        div()
                            .mt_2()
                            .text_size(px(10.0))
                            .text_color(rgb(p.muted))
                            .child("Ctrl+Shift+T 快速打开默认终端"),
                    )
                    .child(
                        div()
                            .mt_5()
                            .text_size(px(11.0))
                            .text_color(rgb(p.muted))
                            .child("本机 Shell   ·   独立会话   ·   你的工作方式"),
                    ),
            )
            .into_any_element()
    }
    fn status_bar(&self, cx: &App) -> AnyElement {
        let p = Palette::new(self.light);
        let entry = self.active_id().and_then(|id| self.panes.get(&id));
        let cwd = entry
            .map(|e| e.spec.cwd.display().to_string())
            .unwrap_or_default();
        let pane_count = self
            .tabs
            .get(self.active)
            .map_or(0, |t| t.layout.ids().len());
        let state = entry
            .map(|e| e.view.read(cx).phase_label())
            .unwrap_or("就绪");
        div()
            .flex()
            .items_center()
            .h(px(26.0))
            .flex_shrink_0()
            .px_3()
            .gap_3()
            .bg(rgb(p.surface))
            .border_t_1()
            .border_color(rgb(p.line))
            .text_size(px(10.0))
            .text_color(rgb(p.muted))
            .child(div().size(px(5.0)).rounded_full().bg(rgb(p.accent)))
            .child(state)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(if self.status.is_empty() {
                        cwd
                    } else {
                        self.status.clone()
                    }),
            )
            .child(div().flex_shrink_0().child(format!("{pane_count} 个窗格")))
            .child(
                div()
                    .flex_shrink_0()
                    .child("Ctrl+Shift+T 新建 · Ctrl+, 设置"),
            )
            .into_any_element()
    }
    fn settings_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let settings = self.settings.as_ref().expect("settings visible");
        let terminal = &settings.terminal;
        let theme = div()
            .flex()
            .gap_2()
            .items_center()
            .child(div().w(px(90.0)).child("主题"))
            .children(
                [("system", "跟随系统"), ("dark", "深色"), ("light", "浅色")]
                    .into_iter()
                    .map(|(value, label)| {
                        self.button(
                            value,
                            if terminal.theme == value {
                                format!("● {label}")
                            } else {
                                label.into()
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(settings) = &mut this.settings {
                                settings.terminal.theme = value.into();
                            }
                            cx.notify();
                        }))
                    }),
            );
        div().flex().flex_col().gap_4()
            .child(div().text_lg().font_weight(FontWeight::BOLD).child("设置"))
            .child(theme)
            .child(div().flex().items_center().gap_2().child(div().w(px(90.0)).child("字体")).child(settings.font.clone()))
            .child(div().flex().items_center().gap_3().child(div().w(px(90.0)).child("字号"))
                .child(self.button("font-smaller","−").on_click(cx.listener(|this,_,_,cx|{if let Some(s)=&mut this.settings{s.terminal.font_size_px=(s.terminal.font_size_px-1.0).max(8.0);}cx.notify();})))
                .child(format!("{} px",terminal.font_size_px))
                .child(self.button("font-larger","＋").on_click(cx.listener(|this,_,_,cx|{if let Some(s)=&mut this.settings{s.terminal.font_size_px=(s.terminal.font_size_px+1.0).min(48.0);}cx.notify();}))))
            .child(div().flex().items_center().gap_3().child(div().w(px(90.0)).child("历史行数"))
                .child(self.button("history-less","− 1000").on_click(cx.listener(|this,_,_,cx|{if let Some(s)=&mut this.settings{s.terminal.scrollback_lines=s.terminal.scrollback_lines.saturating_sub(1000);}cx.notify();})))
                .child(terminal.scrollback_lines.to_string())
                .child(self.button("history-more","＋ 1000").on_click(cx.listener(|this,_,_,cx|{if let Some(s)=&mut this.settings{s.terminal.scrollback_lines=(s.terminal.scrollback_lines+1000).min(20000);}cx.notify();}))))
            .child(div().text_xs().text_color(rgb(p.muted)).child("完整配置可编辑 Profiles、默认 Shell、快捷键、关闭确认及启动恢复。保存后重新加载；Shell 设置仅影响新会话。"))
            .child(div().text_xs().child(self.paths.config.display().to_string()))
            .child(div().flex().gap_2()
                .child(self.button("config-open","编辑完整配置").on_click(cx.listener(|this,_,_,cx|this.edit_config(cx))))
                .child(self.button("config-reload","重新加载").on_click(cx.listener(|this,_,window,cx|this.reload(window,cx)))))
            .child(div().text_color(rgb(p.danger)).child(self.status.clone()))
            .child(div().flex().justify_end().gap_2()
                .child(self.button("settings-cancel","取消").on_click(cx.listener(|this,_,window,cx|this.cancel_modal(window,cx))))
                .child(self.button("settings-save",if self.config_busy{"保存中…"}else{"保存外观"}).on_click(cx.listener(|this,_,window,cx|this.save_settings(window,cx)))))
            .into_any_element()
    }
    fn modal_panel(&self, cx: &mut Context<Self>) -> AnyElement {
        let p = Palette::new(self.light);
        let body=match self.modal.as_ref().expect("modal visible"){
            Modal::Settings=>self.settings_panel(cx),
            Modal::Close(target)=>{let target=*target;
                div().flex().flex_col().gap_4().child(div().text_lg().child("结束运行中的会话？"))
                    .child("关闭将结束选中会话及其 Job 内的进程。尚未保存的 Shell 工作可能丢失。")
                    .child(div().flex().justify_end().gap_2().child(self.button("close-cancel","取消").on_click(cx.listener(|this,_,window,cx|this.cancel_modal(window,cx))))
                        .child(self.button("close-confirm","结束并关闭").on_click(cx.listener(move|this,_,window,cx|this.close_confirmed(target,window,cx))))).into_any_element()
            }
            Modal::Restore=>div().flex().flex_col().gap_4().child(div().text_lg().child("恢复上次的工作区？"))
                .child("将恢复标签、分屏布局及启动目录，并启动新的 Shell 进程。Shell 启动脚本可能执行。")
                .child(div().flex().justify_end().gap_2().child(self.button("restore-cancel","返回欢迎页").on_click(cx.listener(|this,_,window,cx|this.cancel_modal(window,cx))))
                    .child(self.button("restore-confirm","恢复工作区").on_click(cx.listener(|this,_,window,cx|this.restore(window,cx))))).into_any_element(),
            Modal::Rename(input)=>div().flex().flex_col().gap_4().child(div().text_lg().child("重命名标签")).child(input.clone())
                .child(div().text_xs().child("Enter 保存 · Escape 取消"))
                .child(self.button("rename-cancel","取消").on_click(cx.listener(|this,_,window,cx|this.cancel_modal(window,cx)))).into_any_element(),
        };
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000090))
            .occlude()
            .child(
                div()
                    .w(px(680.0))
                    .max_w_full()
                    .max_h_full()
                    .p_6()
                    .rounded_lg()
                    .bg(rgb(p.elevated))
                    .text_color(rgb(p.text))
                    .child(body),
            )
            .into_any_element()
    }
}
impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = Palette::new(self.light);
        let title = if self.tabs.is_empty() {
            "Terminder".into()
        } else {
            format!("{} — Terminder", self.tab_title(self.active, cx))
        };
        window.set_window_title(&title);
        let content = if let Some(tab) = self.tabs.get(self.active) {
            self.render_layout(tab.layout.clone(), vec![], cx)
        } else {
            self.welcome(window, cx)
        };
        div()
            .id("workspace")
            .relative()
            .key_context("Workspace")
            .flex()
            .flex_col()
            .size_full()
            .font_family("Segoe UI")
            .text_sm()
            .bg(rgb(p.surface))
            .text_color(rgb(p.text))
            .on_action(cx.listener(Self::command))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.profile_menu.is_some() {
                    this.profile_key(event, window, cx);
                    return;
                }
                if event.keystroke.key == "escape" {
                    if this.modal.is_some() {
                        this.cancel_modal(window, cx);
                    } else if this.search.is_some() {
                        this.close_search(window, cx);
                    }
                }
                if event.keystroke.key == "enter" {
                    match this.modal {
                        Some(Modal::Close(target)) => this.close_confirmed(target, window, cx),
                        Some(Modal::Restore) => this.restore(window, cx),
                        _ => {}
                    }
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    this.dragging = None;
                    return;
                }
                if let Some(drag) = &this.dragging {
                    let ratio = match drag.axis {
                        Axis::Horizontal => {
                            (event.position.x - drag.bounds.left()) / drag.bounds.size.width
                        }
                        Axis::Vertical => {
                            (event.position.y - drag.bounds.top()) / drag.bounds.size.height
                        }
                    };
                    if let Some(tab) = this.tabs.get_mut(this.active) {
                        tab.layout.set_ratio(&drag.path, ratio.clamp(0.1, 0.9));
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.dragging = None),
            )
            // A root-sized tracked focus target calls prevent_default on every
            // mouse down, cancelling Windows caption actions before they run.
            // Keep keyboard routing on a non-hit-testable anchor instead.
            .child(div().absolute().size_0().track_focus(&self.focus))
            .child(self.title_bar(window, cx))
            .child(self.tab_bar(window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .p_1()
                    .child(content),
            )
            .child(self.status_bar(cx))
            .when(self.search.is_some(), |s| s.child(self.search_bar(cx)))
            .when(self.profile_menu.is_some(), |s| {
                s.child(div().absolute().inset_0().occlude().on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.dismiss_profiles(window, cx);
                        cx.stop_propagation();
                    }),
                ))
            })
            .when(self.modal.is_some(), |s| s.child(self.modal_panel(cx)))
            .when(self.quitting, |s| {
                s.child(
                    div()
                        .absolute()
                        .inset_0()
                        .occlude()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(rgba(0x10151ce0))
                        .text_color(rgb(0xe2e8f0))
                        .child(self.status.clone()),
                )
            })
    }
}
