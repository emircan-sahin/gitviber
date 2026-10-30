//! The tab's bars and its page area: address and tools, device, find, downloads, console, and
//! the area the page's view lies over (or its picture, a placeholder, a device's frame).

use super::*;
use gitviber_ui::widgets::{chord_kbd, command_kbd, find_option_kbd};
use gitviber_ui::Settings;
use gpui::{MouseButton, ObjectFit, StyledImage as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Textarea;
use gpui_kit::component::menu::PopupMenu;

/// Which edge of the page area an Ask box is placed from, and how far.
#[derive(Debug, PartialEq)]
enum Edge {
    Top(f32),
    Bottom(f32),
}

/// Below its anchor while the box fits; else on the area's bottom edge, so a question that grows
/// grows up; at the top when the area is shorter than the box.
fn ask_edge(at: f32, height: f32, area: f32) -> Edge {
    if at + height <= area {
        Edge::Top(at)
    } else if height + 8. <= area {
        Edge::Bottom(4.)
    } else {
        Edge::Top(4.)
    }
}

/// The page's px from its viewport's top left, in the tab's px: where a pick's mark goes.
struct Mark {
    left: f32,
    top: f32,
    w: f32,
    h: f32,
}

/// A find box's option key (⌥⌘C, ⌥⌘W, ⌥⌘R; Alt+ elsewhere): its letter.
fn option_key(k: &gpui::Keystroke) -> Option<char> {
    let m = &k.modifiers;
    let held = if cfg!(target_os = "macos") {
        m.alt && m.platform && !m.control && !m.shift
    } else {
        m.alt && !m.control && !m.platform && !m.shift
    };
    let c = k
        .key
        .chars()
        .next()
        .filter(|_| k.key.chars().count() == 1)?;
    (held && matches!(c, 'c' | 'w' | 'r')).then_some(c)
}

/// The ports menu: what this worktree's terminals serve, or that it's being read (None).
fn port_items(
    mut menu: PopupMenu,
    tab: &WeakEntity<BrowserTab>,
    ports: Option<&[Port]>,
) -> PopupMenu {
    menu = menu.label(match ports {
        None => "Looking for ports…",
        Some([]) => "No terminal here serves a port",
        Some(_) => "Served by this worktree's terminals",
    });
    for p in ports.unwrap_or_default() {
        let tab = tab.clone();
        let url = url::port_url(p.port);
        menu = menu.item(
            PopupMenuItem::new(format!("localhost:{}  {}", p.port, p.process)).on_click(
                move |_, window, cx| {
                    let _ = tab.update(cx, |this, cx| this.open_url(&url, window, cx));
                },
            ),
        );
    }
    menu
}

/// A phone's status bar, upright: the time on the left, signal, Wi-Fi and battery on the right,
/// the camera between them, each a little below the middle as phones draw them.
fn status_bar(f: &Fitted) -> AnyElement {
    let (top, w) = (f.bars.0 as f32, f.screen.w as f32);
    let glyph = (top * 0.32).min(17. * f.scale as f32);
    let third = || {
        h_flex()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(w / 3.))
            .pt(px(top * 0.15))
            .justify_center()
    };
    let signal = h_flex()
        .items_end()
        .gap(px(glyph * 0.12))
        .children([0.3, 0.5, 0.7, 0.9].map(|h| {
            div()
                .w(px(glyph * 0.18))
                .h(px(glyph * h * 0.8))
                .rounded(px(glyph * 0.05))
                .bg(gpui::black())
        }));
    let battery = h_flex()
        .items_center()
        .child(
            div()
                .w(px(glyph * 1.5))
                .h(px(glyph * 0.72))
                .p(px(glyph * 0.1))
                .rounded(px(glyph * 0.2))
                .border_1()
                .border_color(gpui::black().opacity(0.4))
                .child(
                    div()
                        .size_full()
                        .rounded(px(glyph * 0.08))
                        .bg(gpui::black()),
                ),
        )
        .child(
            div()
                .w(px(glyph * 0.1))
                .h(px(glyph * 0.25))
                .rounded_r(px(glyph * 0.05))
                .bg(gpui::black().opacity(0.4)),
        );
    div()
        .absolute()
        .top_0()
        .left_0()
        .w_full()
        .h(px(top))
        .text_color(gpui::black())
        .child(
            third()
                .left_0()
                .text_size(px(glyph))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("9:41"),
        )
        .child(
            third()
                .right_0()
                .gap(px(glyph * 0.3))
                .child(signal)
                .child(Icon::new(IconName::Wifi).size(px(glyph * 0.85)))
                .child(battery),
        )
        .into_any_element()
}

fn icon_button(id: &'static str, icon: IconName, tip: impl Into<SharedString>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(Icon::new(icon))
        .tooltip(tip)
}

fn placeholder(
    t: &AppTheme,
    title: impl Into<SharedString>,
    detail: Option<SharedString>,
    action: Option<Button>,
) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_2()
        .p_4()
        .child(
            div()
                .text_size(px(13.))
                .text_color(t.foreground)
                .child(title.into()),
        )
        .children(detail.map(|d| {
            div()
                .max_w(px(420.))
                .text_size(px(11.5))
                .text_color(t.muted_foreground)
                .child(d)
        }))
        .children(action)
        .into_any_element()
}

impl Focusable for BrowserTab {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TabItem for BrowserTab {
    fn tab_title(&self, cx: &App) -> SharedString {
        self.page_title(cx)
            .unwrap_or_else(|| page_label(&self.current_url(cx)))
            .into()
    }

    /// A page named by its title has its host beside it.
    fn tab_detail(&self, cx: &App) -> Option<SharedString> {
        self.page_title(cx)?;
        let url = self.current_url(cx);
        let host = page_host(&url);
        (!host.is_empty()).then(|| host.to_string().into())
    }

    fn tab_icon(&self, cx: &App) -> TabIcon {
        let agents = self
            .page_info(cx)
            .is_some_and(|p| p.agent.is_some() && !p.taken && !p.released);
        TabIcon::Icon(if agents {
            IconName::Bot
        } else {
            IconName::Globe
        })
    }

    fn tab_tooltip(&self, cx: &App) -> Option<SharedString> {
        let page = page_label(&self.current_url(cx));
        let info = self.page_info(cx);
        Some(match info {
            Some(p) if p.shown_by.is_some() => {
                format!("{page}\nOpened for you by an agent in a terminal").into()
            }
            Some(p) if p.agent.is_some() && !p.released => {
                format!("{page}\nOpened by the agent in a terminal pane").into()
            }
            _ => page.into(),
        })
    }

    fn tab_focus(&self, _: &App) -> Option<FocusHandle> {
        Some(if self.page_had_keys {
            self.page_focus.clone()
        } else {
            self.focus.clone()
        })
    }

    /// An agent's page while Settings → Browser → Show agents' tabs is off, unless the user took
    /// it or its pane closed: it keeps running out of the strip.
    fn hidden(&self, cx: &App) -> bool {
        !Settings::get(cx).browser_show_agent_tabs
            && self
                .page_info(cx)
                .is_some_and(|p| p.agent.is_some() && !p.taken && !p.released)
    }

    /// A page an agent opened can't come back as it was.
    fn reopenable(&self, cx: &App) -> bool {
        self.page_info(cx)
            .is_none_or(|p| p.agent.is_none() && p.shown_by.is_none())
    }
}

impl BrowserTab {
    fn address_bar(&mut self, t: &AppTheme, cx: &mut Context<Self>) -> AnyElement {
        let state = self.state(cx).cloned();
        let current = self.current_url(cx);
        let can_back = state.as_ref().is_some_and(|s| s.can_back);
        let can_forward = state.as_ref().is_some_and(|s| s.can_forward);
        let loading = state.as_ref().is_some_and(|s| s.loading);
        let progress = state.as_ref().map_or(0.0, |s| s.progress);
        let insecure = state.as_ref().is_some_and(|s| s.insecure);
        let info = self.page_info(cx);
        let taken = info.is_some_and(|p| p.taken);
        let (errors, warnings) = info.map_or((0, 0), |p| (p.errors, p.warnings));
        let capture = Settings::get(cx).browser_console;
        let reload = if loading {
            icon_button("stop", IconName::X, "Stop")
                .on_click(cx.listener(|this, _, window, cx| this.go(Go::Stop, window, cx)))
        } else {
            icon_button(
                "reload",
                IconName::RotateCw,
                "Reload (⇧-click: past the cache)",
            )
            .on_click(cx.listener(|this, e: &ClickEvent, window, cx| {
                let hard = e.modifiers().shift;
                this.go(if hard { Go::HardReload } else { Go::Reload }, window, cx)
            }))
        };
        let weak = cx.weak_entity();
        let ports_menu = icon_button("ports", IconName::Network, "Ports the terminals serve")
            .dropdown_menu(move |menu, window, cx| {
                // Read as it opens, filled in once known: a dev server just started is found.
                let tab = weak.clone();
                cx.spawn_in(window, async move |menu, cx| {
                    let Ok(read) = tab.update_in(cx, |tab, window, cx| tab.read_ports(window, cx))
                    else {
                        return;
                    };
                    let ports = read.await;
                    let _ = menu.update_in(cx, |menu, window, cx| {
                        menu.rebuild(window, cx, |menu, _, _| {
                            port_items(menu, &tab, Some(&ports))
                        })
                    });
                })
                .detach();
                port_items(menu, &weak, None)
            });
        let console = capture.then(|| {
            let count = if errors > 0 { errors } else { warnings };
            let (icon, tip) = if errors > 0 {
                (IconName::CircleX, format!("{errors} errors on the page"))
            } else if warnings > 0 {
                (
                    IconName::TriangleAlert,
                    format!("{warnings} warnings on the page"),
                )
            } else {
                (IconName::TriangleAlert, "The page's console".to_string())
            };
            let mut b = Button::new("console")
                .ghost()
                .xsmall()
                .icon(Icon::new(icon))
                .tooltip(tip)
                .selected(self.console_open)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_console(cx)));
            if count > 0 {
                b = b.label(if count > 99 {
                    "99+".to_string()
                } else {
                    count.to_string()
                });
            }
            b
        });
        let scheme = match self.saved.scheme {
            None => (IconName::SunMoon, "Page as the app (click for light)"),
            Some(Scheme::Light) => (IconName::Sun, "Page light (click for dark)"),
            Some(Scheme::Dark) => (IconName::Moon, "Page dark (click for as the app)"),
        };
        let zoom = (self.saved.device.is_none() && self.saved.zoom != 1.0).then(|| {
            Button::new("zoom")
                .ghost()
                .xsmall()
                .label(devices::zoom_label(self.saved.zoom))
                .tooltip("Actual Size")
                .on_click(cx.listener(|this, _, window, cx| this.zoom_by(0, window, cx)))
        });
        let width = relative(progress.max(0.05) as f32);
        h_flex()
            .relative()
            .h(px(34.))
            .flex_none()
            .px(px(6.))
            .gap(px(2.))
            .border_b_1()
            .border_color(t.border)
            .child(
                icon_button("back", IconName::ArrowLeft, "Back")
                    .disabled(!can_back)
                    .on_click(cx.listener(|this, _, window, cx| this.go(Go::Back, window, cx))),
            )
            .child(
                icon_button("forward", IconName::ArrowRight, "Forward")
                    .disabled(!can_forward)
                    .on_click(cx.listener(|this, _, window, cx| this.go(Go::Forward, window, cx))),
            )
            .child(reload)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .mx(px(4.))
                    .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.editing = false;
                        this.keys_to_page(window, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        Input::new(&self.address)
                            .xsmall()
                            .font_family(t.fonts.code.clone()),
                    ),
            )
            .when(self.invalid, |bar| {
                bar.child(
                    div()
                        .mr(px(4.))
                        .text_size(px(11.5))
                        .text_color(t.destructive)
                        .child("Not an address"),
                )
            })
            .when(insecure, |bar| {
                bar.child(
                    div()
                        .id("insecure")
                        .px(px(4.))
                        .text_color(t.modified)
                        .child(Icon::new(IconName::ShieldAlert).size(px(14.)))
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "Its self-signed certificate is let through on this machine only",
                            )
                            .build(window, cx)
                        }),
                )
            })
            .when(taken, |bar| {
                bar.child(
                    Button::new("give-back")
                        .ghost()
                        .xsmall()
                        .icon(Icon::new(IconName::Bot))
                        .label("Give back to agent")
                        .tooltip("Its agents only read the page while it's yours")
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(handle) = handle() {
                                if let Err(e) = crate::agent_give_back(handle, &this.saved.id) {
                                    this.toast(
                                        Toast::error(e).title("Could not give the tab back"),
                                        window,
                                        cx,
                                    );
                                }
                            }
                        })),
                )
            })
            .child(
                icon_button(
                    "pick",
                    IconName::Crosshair,
                    if self.picking {
                        "Stop picking"
                    } else {
                        "Pick an element for the agent"
                    },
                )
                .tooltip_key_binding(if self.picking {
                    chord_kbd("escape")
                } else {
                    command_kbd("browser.pick", cx)
                })
                .selected(self.picking)
                .on_click(cx.listener(|this, _, window, cx| {
                    let on = !this.picking;
                    this.set_picking(on, window, cx)
                })),
            )
            .children(console)
            .children(zoom)
            .child(
                icon_button("scheme", scheme.0, scheme.1)
                    .selected(self.saved.scheme.is_some())
                    .on_click(cx.listener(|this, _, window, cx| this.cycle_scheme(window, cx))),
            )
            .child(
                icon_button(
                    "device",
                    IconName::TabletSmartphone,
                    if self.saved.device.is_some() {
                        "Leave device mode"
                    } else {
                        "Device mode"
                    },
                )
                .tooltip_key_binding(command_kbd("browser.toggleDevice", cx))
                .selected(self.saved.device.is_some())
                .on_click(cx.listener(|this, _, window, cx| this.toggle_device(window, cx))),
            )
            .child(ports_menu)
            .child(
                icon_button("external", IconName::ExternalLink, "Open in Browser")
                    .disabled(current == BLANK)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let url = this.current_url(cx);
                        if let Err(e) = gitviber_core::launch::open_url(&url) {
                            this.toast(
                                Toast::error(e).title("Could not open the link"),
                                window,
                                cx,
                            );
                        }
                    })),
            )
            .when(loading, |bar| {
                bar.child(
                    div()
                        .absolute()
                        .bottom_0()
                        .left_0()
                        .h(px(1.))
                        .w(width)
                        .bg(t.primary),
                )
            })
            .into_any_element()
    }

    fn device_bar(
        &mut self,
        t: &AppTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (device, choice, fitted) = self.fitted()?;
        let responsive = choice.name == devices::RESPONSIVE;
        let sizes = if responsive {
            self.size_fields((device.w, device.h), window, cx)
        } else {
            self.size_fields = None;
            None
        };
        let weak = cx.weak_entity();
        let rotated = choice.rotated;
        let current = choice.name.clone();
        let picker = Button::new("device-name")
            .ghost()
            .xsmall()
            .label(device.name.clone())
            .dropdown_caret(true)
            .dropdown_menu(move |mut menu, _, _| {
                let names = std::iter::once(devices::RESPONSIVE.to_string())
                    .chain(devices::DEVICES.iter().map(|d| d.name.clone()));
                for (i, name) in names.enumerate() {
                    if i == 1 {
                        menu = menu.separator();
                    }
                    let weak = weak.clone();
                    let checked = name == current;
                    menu = menu.item(PopupMenuItem::new(name.clone()).checked(checked).on_click(
                        move |_, window, cx| {
                            let next = DeviceChoice {
                                name: name.clone(),
                                rotated,
                                ..DeviceChoice::default()
                            };
                            let _ = weak
                                .update(cx, |this, cx| this.choose_device(Some(next), window, cx));
                        },
                    ));
                }
                // Two lines: a menu's label doesn't wrap.
                menu.separator()
                    .label("Still your Mac's browser engine: <meta name=viewport>")
                    .label("is ignored, :hover follows your pointer.")
            });
        let viewport = format!(
            "{}×{}{}",
            fitted.viewport.0.round(),
            fitted.viewport.1.round(),
            if device.dpr > 0.0 {
                format!(" @{}x", device.dpr)
            } else {
                String::new()
            }
        );
        Some(
            h_flex()
                .h(px(30.))
                .flex_none()
                .px(px(8.))
                .gap(px(4.))
                .border_b_1()
                .border_color(t.border)
                .child(picker)
                .children(sizes.map(|(w, h)| {
                    h_flex()
                        .gap(px(4.))
                        .font_family(t.fonts.code.clone())
                        .text_size(px(11.5))
                        .text_color(t.muted_foreground)
                        .child(div().w(px(64.)).child(Input::new(&w).xsmall()))
                        .child("×")
                        .child(div().w(px(64.)).child(Input::new(&h).xsmall()))
                }))
                .when(!responsive, |bar| {
                    bar.child(
                        icon_button("rotate", IconName::RotateCcwSquare, "Rotate").on_click(
                            cx.listener(|this, _, window, cx| {
                                if let Some(mut c) = this.saved.device.clone() {
                                    c.rotated = !c.rotated;
                                    this.choose_device(Some(c), window, cx);
                                }
                            }),
                        ),
                    )
                })
                .child(
                    div()
                        .px(px(4.))
                        .font_family(t.fonts.code.clone())
                        .text_size(px(11.5))
                        .text_color(t.subtle)
                        .child(viewport),
                )
                .when(!self.dpr && device.dpr > 0.0, |bar| {
                    bar.child(
                        div()
                            .px(px(4.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(t.border)
                            .text_size(px(10.))
                            .text_color(t.subtle)
                            .child("DPR n/a"),
                    )
                })
                .into_any_element(),
        )
    }

    fn find_bar(&mut self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let f = self.find.as_ref()?;
        let missing = f.found == Some(false);
        Some(
            h_flex()
                .h(px(32.))
                .flex_none()
                .justify_end()
                .px(px(8.))
                .gap(px(2.))
                .border_b_1()
                .border_color(t.border)
                .capture_action(cx.listener(|this, _: &Escape, window, cx| {
                    this.close_find(window, cx);
                    cx.stop_propagation();
                }))
                // The find boxes' option keys: ⌥⌘C flips Match Case; WebKit's find has no
                // whole word or regex, so ⌥⌘W and ⌥⌘R do nothing here.
                .capture_key_down(cx.listener(|this, e: &gpui::KeyDownEvent, _, cx| {
                    let Some(key) = option_key(&e.keystroke) else {
                        return;
                    };
                    cx.stop_propagation();
                    if key == 'c' {
                        this.toggle_match_case(cx);
                    }
                }))
                .child(div().w(px(240.)).child(Input::new(&f.input).xsmall()))
                .when(missing, |bar| {
                    bar.child(
                        div()
                            .px(px(4.))
                            .text_size(px(11.5))
                            .text_color(t.destructive)
                            .child("No results"),
                    )
                })
                .child(
                    icon_button("find-case", IconName::CaseSensitive, "Match Case")
                        .tooltip_key_binding(find_option_kbd('c'))
                        .selected(f.match_case)
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_match_case(cx))),
                )
                .child(
                    icon_button("find-prev", IconName::ChevronUp, "Previous")
                        .tooltip_key_binding(command_kbd("browser.findPrev", cx))
                        .on_click(cx.listener(|this, _, _, cx| this.run_find(true, cx))),
                )
                .child(
                    icon_button("find-next", IconName::ChevronDown, "Next")
                        .tooltip_key_binding(command_kbd("browser.findNext", cx))
                        .on_click(cx.listener(|this, _, _, cx| this.run_find(false, cx))),
                )
                .child(
                    icon_button("find-close", IconName::X, "Close")
                        .on_click(cx.listener(|this, _, window, cx| this.close_find(window, cx))),
                )
                .into_any_element(),
        )
    }

    fn download_bar(&mut self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let downloads = self.page_info(cx)?.downloads.clone();
        if downloads.is_empty() {
            return None;
        }
        let mut rows = v_flex().flex_none().border_b_1().border_color(t.border);
        for d in downloads {
            let going = d.state == "going";
            let text = format::download_text(&d);
            let key = d.key;
            let path = d.path.clone();
            let icon = if d.state == "failed" {
                IconName::CircleAlert
            } else {
                IconName::Download
            };
            rows = rows.child(
                h_flex()
                    .h(px(26.))
                    .px(px(12.))
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(
                        Icon::new(icon)
                            .size(px(14.))
                            .text_color(if d.state == "failed" {
                                t.destructive
                            } else {
                                t.muted_foreground
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(260.))
                            .truncate()
                            .child(d.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(t.muted_foreground)
                            .child(text),
                    )
                    .when(going, |row| {
                        row.child(
                            Button::new(("cancel-download", key as usize))
                                .ghost()
                                .xsmall()
                                .label("Cancel")
                                .on_click(move |_, _, _| drop(crate::cancel_download(key))),
                        )
                    })
                    .when(d.state == "done" && path.is_some(), |row| {
                        let path = path.clone().unwrap_or_default();
                        row.child(
                            Button::new(("reveal-download", key as usize))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::FolderOpen))
                                .tooltip("Show in Finder")
                                .on_click(move |_, _, _| {
                                    let _ = gitviber_core::launch::reveal(path.clone().into());
                                }),
                        )
                    })
                    .when(!going, |row| {
                        row.child(
                            Button::new(("dismiss-download", key as usize))
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::X))
                                .tooltip("Dismiss")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let id = this.saved.id.clone();
                                    if let Some(hub) = &this.hub {
                                        hub.update(cx, |hub, cx| {
                                            hub.dismiss_download(&id, key, cx)
                                        });
                                    }
                                })),
                        )
                    }),
            );
        }
        Some(rows.into_any_element())
    }

    fn console_panel(&mut self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.console_open || !Settings::get(cx).browser_console {
            return None;
        }
        let entries = &self.console;
        let errors = entries.iter().any(|e| e.level == "error");
        let shown = &entries[entries.len().saturating_sub(100)..];
        let mut list = v_flex()
            .id("console-lines")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(8.))
            .pb(px(6.))
            .font_family(t.fonts.code.clone())
            .text_size(px(11.5));
        if shown.is_empty() {
            list = list.child(
                div()
                    .py(px(8.))
                    .text_color(t.subtle)
                    .child("Nothing logged since the console was last cleared."),
            );
        }
        for e in shown {
            list = list.child(if e.level == "load" {
                div()
                    .mt(px(4.))
                    .pt(px(4.))
                    .border_t_1()
                    .border_color(t.border)
                    .text_size(px(10.5))
                    .text_color(t.subtle)
                    .child(page_label(&e.url))
                    .into_any_element()
            } else {
                let error = e.level == "error";
                h_flex()
                    .items_start()
                    .gap(px(6.))
                    .py(px(2.))
                    .text_color(if error { t.destructive } else { t.modified })
                    .child(
                        Icon::new(if error {
                            IconName::CircleX
                        } else {
                            IconName::TriangleAlert
                        })
                        .size(px(12.)),
                    )
                    .child(div().min_w_0().flex_1().child(e.msg.clone()))
                    .into_any_element()
            });
        }
        Some(
            v_flex()
                .flex_none()
                .max_h(px(224.))
                .border_b_1()
                .border_color(t.border)
                .child(
                    h_flex()
                        .h(px(30.))
                        .flex_none()
                        .px(px(8.))
                        .gap(px(4.))
                        .child(div().mr_auto().text_size(px(12.)).child("Console"))
                        .child(
                            Button::new("send-errors")
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Send))
                                .label("Send Errors to Agent")
                                .disabled(!errors)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let text = this.errors_text(cx);
                                    if !text.is_empty() {
                                        this.picks = None;
                                        this.open_ask(
                                            format::errors_about(&text),
                                            (8., 8.),
                                            window,
                                            cx,
                                        );
                                    }
                                })),
                        )
                        .child(
                            Button::new("clear-console")
                                .ghost()
                                .xsmall()
                                .icon(Icon::new(IconName::Trash))
                                .label("Clear")
                                .disabled(entries.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let _ = crate::console_clear(&this.saved.id);
                                    this.console.clear();
                                    cx.notify();
                                })),
                        )
                        .child(
                            icon_button("close-console", IconName::X, "Close")
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_console(cx))),
                        ),
                )
                .child(list)
                .into_any_element(),
        )
    }

    /// The area the page's view lies over, reporting its bounds to the placer as it paints.
    fn page_canvas(
        &self,
        screen: Option<Screen>,
        live: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = self.saved.id.clone();
        let tab = cx.weak_entity();
        canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
            move |bounds: Bounds<Pixels>, hitbox: Hitbox, window, cx| {
                placer::painted(
                    Painted {
                        id,
                        bounds,
                        hitbox,
                        screen,
                        live,
                        tab,
                    },
                    window,
                    cx,
                )
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// The area's size, kept for device mode's fit: a change lays the tab out again.
    fn measure(&self) -> AnyElement {
        let area = self.area.clone();
        canvas(
            move |bounds, window, _| {
                let size = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                if area.replace(size) != size {
                    window.request_animation_frame();
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    fn picture(&self) -> Option<AnyElement> {
        let image = self.cover.clone().or_else(|| self.restoring.clone())?;
        Some(
            img(image)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
                .into_any_element(),
        )
    }

    /// What shows of the page, which a pick's mark stays inside: its screen in device mode, else
    /// the whole area (a mark ran off the device's screen).
    pub(super) fn mark_room(&self) -> (f32, f32) {
        self.fitted().map_or(self.area.get(), |(_, _, f)| {
            (f.page.w as f32, f.page.h as f32)
        })
    }

    fn marks(&self, t: &AppTheme) -> Vec<AnyElement> {
        let Some(picks) = &self.picks else {
            return Vec::new();
        };
        let factor = self.factor() as f32;
        let area = self.mark_room();
        picks
            .iter()
            .map(|p| {
                let b = p.bounds;
                let within = |v: f32, most: f32| v.clamp(0., most.max(0.));
                let left = within(b.x as f32 * factor, area.0 - 2.);
                let top = within(b.y as f32 * factor, area.1 - 2.);
                let right = within((b.x + b.w) as f32 * factor, area.0);
                let bottom = within((b.y + b.h) as f32 * factor, area.1);
                Mark {
                    left,
                    top,
                    w: (right - left).max(2.),
                    h: (bottom - top).max(2.),
                }
            })
            .map(|m| {
                div()
                    .absolute()
                    .left(px(m.left))
                    .top(px(m.top))
                    .w(px(m.w))
                    .h(px(m.h))
                    .rounded(px(2.))
                    .border_2()
                    .border_color(t.primary)
                    .bg(t.primary.opacity(0.15))
                    .into_any_element()
            })
            .collect()
    }

    fn ask_box(&self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let ask = self.ask.as_ref()?;
        let area = self.area.get();
        let w = 320.;
        let left = ask.at.0.clamp(4., (area.0 - w - 4.).max(4.));
        let edge = ask_edge(ask.at.1, ask.height.get(), area.1);
        let height = ask.height.clone();
        let errors = self.page_info(cx).map_or(0, |p| p.errors);
        let picks = self.picks.is_some();
        let include = ask.include_errors;
        let model = ask.model.clone();
        let empty = ask.input.read(cx).value().trim().is_empty();
        let agent = super::super::ask::agent_name(cx);
        Some(
            v_flex()
                .id("ask")
                .absolute()
                .left(px(left))
                .map(|b| match edge {
                    Edge::Top(y) => b.top(px(y)),
                    Edge::Bottom(y) => b.bottom(px(y)),
                })
                .w(px(w))
                .gap(px(6.))
                .p(px(8.))
                .rounded(px(8.))
                .border_1()
                .border_color(t.border)
                .bg(t.elevated)
                .shadow_lg()
                .occlude()
                .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                    this.close_ask(cx);
                    cx.stop_propagation();
                }))
                .child(
                    canvas(
                        move |bounds, window, _| {
                            // It fills the padding box: the border adds 1px above and below.
                            let h = f32::from(bounds.size.height) + 2.;
                            if height.replace(h) != h {
                                window.request_animation_frame();
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .child(
                    div()
                        .truncate()
                        .font_family(t.fonts.code.clone())
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(ask.about.summary.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .child(format!("Ask {agent} about {}", ask.about.what)),
                )
                .child(Textarea::new(&ask.input))
                .when(picks && errors > 0, |b| {
                    let label = if errors as usize > format::ERRORS {
                        format!(
                            "Include the latest {} of the page's {errors} errors",
                            format::ERRORS
                        )
                    } else {
                        format!(
                            "Include {errors} page {}",
                            if errors == 1 { "error" } else { "errors" }
                        )
                    };
                    b.child(
                        Checkbox::new("include-errors")
                            .label(label)
                            .checked(include)
                            .on_click(cx.listener(|this, on: &bool, _, cx| {
                                if let Some(a) = this.ask.as_mut() {
                                    a.include_errors = *on;
                                }
                                cx.notify();
                            })),
                    )
                })
                .child(
                    h_flex()
                        .gap(px(4.))
                        .child(match model {
                            Some((choices, value)) => {
                                let weak = cx.weak_entity();
                                let label = if value.is_empty() {
                                    "Default".to_string()
                                } else {
                                    value.clone()
                                };
                                Button::new("ask-model")
                                    .ghost()
                                    .xsmall()
                                    .label(label)
                                    .tooltip("The model it starts with")
                                    .dropdown_caret(true)
                                    .dropdown_menu(move |mut menu, _, _| {
                                        for m in &choices {
                                            let (weak, m) = (weak.clone(), m.clone());
                                            let label = if m.is_empty() {
                                                "Default".to_string()
                                            } else {
                                                m.clone()
                                            };
                                            menu = menu.item(
                                                PopupMenuItem::new(label)
                                                    .checked(m == value)
                                                    .on_click(move |_, _, cx| {
                                                        let _ = weak.update(cx, |this, cx| {
                                                            this.pick_model(m.clone(), cx)
                                                        });
                                                    }),
                                            );
                                        }
                                        menu
                                    })
                                    .into_any_element()
                            }
                            None => div()
                                .text_size(px(11.))
                                .text_color(t.subtle)
                                .child("⇧↵ new line")
                                .into_any_element(),
                        })
                        .child(div().flex_1())
                        .child(
                            Button::new("ask-close")
                                .ghost()
                                .xsmall()
                                .label("Close")
                                .on_click(cx.listener(|this, _, _, cx| this.close_ask(cx))),
                        )
                        .child(
                            Button::new("ask-skip")
                                .ghost()
                                .xsmall()
                                .label("Skip")
                                .tooltip("A session with it waiting in the prompt box")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.send_ask(false, window, cx)
                                })),
                        )
                        .child(
                            Button::new("ask-send")
                                .primary()
                                .xsmall()
                                .label("Ask")
                                .disabled(empty)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.send_ask(true, window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn page_area(&mut self, t: &AppTheme, cx: &mut Context<Self>) -> AnyElement {
        let failed = self.state(cx).and_then(|s| s.failed.clone());
        let mut keys = gpui::KeyContext::default();
        keys.add(PAGE_CONTEXT);
        keys.set("page", self.saved.id.clone());
        let area = div()
            .id("browser-page")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(t.background)
            .key_context(keys)
            .track_focus(&self.page_focus)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.page_focus.focus(window, cx);
                    cx.notify();
                }),
            )
            .child(self.measure());
        if let Some(error) = self.error.clone() {
            let action = (!cfg!(target_os = "macos") && self.saved.url != BLANK).then(|| {
                let url = self.saved.url.clone();
                Button::new("open-outside")
                    .outline()
                    .small()
                    .label("Open in your browser")
                    .on_click(move |_, _, _| drop(gitviber_core::launch::open_url(&url)))
            });
            return area
                .child(placeholder(
                    t,
                    "The browser can't open here",
                    Some(error),
                    action,
                ))
                .into_any_element();
        }
        if let Some(f) = failed {
            let url = f.url.clone();
            let retry = Button::new("try-again")
                .outline()
                .small()
                .label("Try Again")
                .on_click(cx.listener(move |this, _, window, cx| this.open_url(&url, window, cx)));
            return area
                .child(placeholder(
                    t,
                    format!("Can't open {}", page_label(&f.url)),
                    Some(f.message.into()),
                    Some(retry),
                ))
                .into_any_element();
        }
        let made = self.made.is_some();
        let live = made && self.restoring.is_none();
        let Some((device, choice, fitted)) = self.fitted() else {
            return area
                .child(self.page_canvas(None, live, cx))
                .children(self.picture())
                .children(self.marks(t))
                .children(self.ask_box(t, cx))
                .into_any_element();
        };
        if !fitted.fits {
            return area
                .child(placeholder(
                    t,
                    "Too little room to show the device",
                    None,
                    None,
                ))
                .into_any_element();
        }
        let screen = Screen {
            width: fitted.viewport.0,
            height: fitted.viewport.1,
            radius: fitted.radius,
            corners: fitted.corners,
            dpr: (device.dpr > 0.0).then_some(device.dpr),
        };
        area.child(self.device_frame(t, &device, &fitted))
            .child(
                div()
                    .absolute()
                    .left(px(fitted.page.x as f32))
                    .top(px(fitted.page.y as f32))
                    .w(px(fitted.page.w as f32))
                    .h(px(fitted.page.h as f32))
                    .overflow_hidden()
                    .child(self.page_canvas(Some(screen), live, cx))
                    .children(self.picture())
                    .children(self.marks(t)),
            )
            .children(self.ask_box(t, cx))
            .children((choice.name == devices::RESPONSIVE).then(|| self.handles(t, &fitted, cx)))
            .children(self.drag.is_some().then(|| self.dragging(cx)))
            .into_any_element()
    }

    /// Responsive's right and bottom edges and corner, outside the screen so they show.
    fn handles(&self, t: &AppTheme, f: &Fitted, cx: &mut Context<Self>) -> AnyElement {
        let s = f.screen;
        let (right, bottom) = ((s.x + s.w) as f32 + 4., (s.y + s.h) as f32 + 4.);
        let grip = |id: &'static str, dx: f64, dy: f64, x: f32, y: f32, w: f32, h: f32| {
            div()
                .id(id)
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .rounded_full()
                .bg(t.border_strong)
                .hover(|d| d.bg(t.primary))
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &gpui::MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.start_drag(dx, dy, e.position, cx)
                    }),
                )
        };
        let (cx_mid, cy_mid) = ((s.x + s.w / 2.) as f32, (s.y + s.h / 2.) as f32);
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(grip("grip-right", 1., 0., right, cy_mid - 20., 5., 40.).cursor_ew_resize())
            .child(grip("grip-bottom", 0., 1., cx_mid - 20., bottom, 40., 5.).cursor_ns_resize())
            .child(grip("grip-corner", 1., 1., right, bottom, 8., 8.).cursor_nwse_resize())
            .into_any_element()
    }

    /// While an edge is dragged: the pointer followed wherever it goes, and let go anywhere.
    fn dragging(&self, cx: &mut Context<Self>) -> AnyElement {
        let weak = cx.weak_entity();
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = weak.clone();
                window.on_mouse_event(move |e: &gpui::MouseMoveEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = moving.update(cx, |this, cx| this.drag_to(e.position, cx));
                    }
                });
                let up = weak.clone();
                window.on_mouse_event(move |_: &gpui::MouseUpEvent, phase, window, cx| {
                    if phase == gpui::DispatchPhase::Capture {
                        let _ = up.update(cx, |this, cx| this.end_drag(window, cx));
                    }
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// The device's body and screen around the page: its bezel, status bar, cutout and home
    /// indicator, drawn here as a phone's browser shows them.
    fn device_frame(&self, t: &AppTheme, device: &devices::Device, f: &Fitted) -> AnyElement {
        let body = if t.dark {
            gpui::rgb(0x2a2a2e)
        } else {
            gpui::rgb(0x1c1c1e)
        };
        let (s, frame) = (f.screen_in_frame(), f.frame);
        let (top, left, bottom) = f.bars;
        let mut screen = div()
            .absolute()
            .left(px(s.x as f32))
            .top(px(s.y as f32))
            .w(px(s.w as f32))
            .h(px(s.h as f32))
            .rounded(px(f.radius as f32))
            .overflow_hidden()
            .bg(gpui::white());
        if top > 0.0 || left > 0.0 {
            screen = screen.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .when(top > 0.0, |d| d.w_full().h(px(top as f32)))
                    .when(left > 0.0, |d| d.h_full().w(px(left as f32)))
                    .bg(gpui::rgb(0xf2f2f7)),
            );
        }
        if top > 0.0 {
            screen = screen.child(status_bar(f));
        }
        if bottom > 0.0 {
            screen = screen.child(
                div()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .w_full()
                    .h(px(bottom as f32))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui::rgb(0xf2f2f7))
                    .child(
                        div()
                            .w(px((s.w * 0.35) as f32))
                            .h(px(4.))
                            .rounded(px(2.))
                            .bg(gpui::black()),
                    ),
            );
        }
        if let Some((c, r)) = f.cutout {
            screen = screen.child(
                div()
                    .absolute()
                    .left(px(c.x as f32))
                    .top(px(c.y as f32))
                    .w(px(c.w as f32))
                    .h(px(c.h as f32))
                    .rounded(px(r as f32))
                    .bg(gpui::black()),
            );
        }
        let bezel = device.bezel * f.scale;
        div()
            .absolute()
            .left(px(frame.x as f32))
            .top(px(frame.y as f32))
            .w(px(frame.w as f32))
            .h(px(frame.h as f32))
            .rounded(px((f.radius + bezel) as f32))
            .bg(if bezel > 0.0 {
                body.into()
            } else {
                t.background
            })
            .child(screen)
            .into_any_element()
    }
}

impl Render for BrowserTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_made(window, cx);
        let t = cx.app_theme().clone();
        let picking = self.picking;
        v_flex()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_action(
                cx.listener(|this, _: &act::FocusAddress, window, cx| {
                    this.focus_address(window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &act::Reload, window, cx| this.go(Go::Reload, window, cx)),
            )
            .on_action(cx.listener(|this, _: &act::Back, window, cx| this.go(Go::Back, window, cx)))
            .on_action(
                cx.listener(|this, _: &act::Forward, window, cx| this.go(Go::Forward, window, cx)),
            )
            .on_action(cx.listener(|this, _: &act::Inspect, window, cx| this.inspect(window, cx)))
            .on_action(cx.listener(|this, _: &act::Pick, window, cx| {
                let on = !this.picking;
                this.set_picking(on, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &act::ToggleDevice, window, cx| {
                    this.toggle_device(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &act::Find, window, cx| this.show_find(window, cx)))
            .on_action(
                cx.listener(|this, _: &act::FindNext, window, cx| {
                    this.find_step(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &act::FindPrev, window, cx| this.find_step(true, window, cx)),
            )
            .on_action(cx.listener(|this, _: &act::ZoomIn, window, cx| this.zoom_by(1, window, cx)))
            .on_action(
                cx.listener(|this, _: &act::ZoomOut, window, cx| this.zoom_by(-1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &act::ZoomReset, window, cx| this.zoom_by(0, window, cx)),
            )
            // Esc in the app too: a click outside the page took the keys from it.
            .when(picking, |el| {
                el.capture_action(
                    cx.listener(|this, _: &Escape, window, cx| this.set_picking(false, window, cx)),
                )
            })
            .text_color(t.foreground)
            .child(self.address_bar(&t, cx))
            .children(self.device_bar(&t, window, cx))
            .children(self.find_bar(&t, cx))
            .children(self.download_bar(&t, cx))
            .children(self.console_panel(&t, cx))
            .child(self.page_area(&t, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{BrowserTab, Saved};
    use super::{ask_edge, option_key, Edge};
    use crate::ui::devices::{DeviceChoice, RESPONSIVE};
    use gitviber_ui::workspace::{Selection, Workspace};
    use gpui::{
        div, AnyView, AppContext as _, Context, IntoElement, Keystroke, ParentElement as _, Render,
        Styled as _, TestAppContext, WeakEntity, Window,
    };

    struct Host(AnyView);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.0.clone())
        }
    }

    /// Responsive left while its width field has the focus: the tab has it back, so its keys
    /// work (a field no longer drawn kept it, and ⇧⌘C did nothing).
    #[gpui::test]
    fn leaving_responsive_mid_typing_gives_the_tab_its_keys(cx: &mut TestAppContext) {
        let dir = gitviber_ui::test_support::config_dir("browser-size");
        cx.update(|cx| {
            gitviber_ui::init(cx);
            crate::ui::hub::install(cx, |_, _| {});
        });
        let responsive = serde_json::json!({ "name": RESPONSIVE, "w": 400.0, "h": 800.0 });
        let saved = Saved::of(&Selection::Browser {
            id: crate::ui::new_id(),
            url: "http://127.0.0.1:9/".into(),
            title: None,
            device: Some(responsive),
            zoom: None,
            scheme: None,
        })
        .unwrap();
        let mut tab = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let t = cx.new(|cx| {
                BrowserTab::new(
                    saved,
                    "/tmp".into(),
                    WeakEntity::<Workspace>::new_invalid(),
                    window,
                    cx,
                )
            });
            tab = Some(t.clone());
            Host(t.into())
        });
        let tab = tab.unwrap();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let field = tab.read_with(cx, |t, _| t.size_fields.as_ref().map(|f| f.w.clone()));
        let field = field.expect("Responsive's fields");
        cx.update(|window, cx| field.update(cx, |f, cx| f.focus(window, cx)));
        tab.update_in(cx, |t, window, cx| {
            t.choose_device(Some(DeviceChoice::default_device()), window, cx)
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        let back = tab.update_in(cx, |t, window, cx| {
            t.size_fields.is_none() && t.focus.contains_focused(window, cx)
        });
        assert!(back, "the tab has the focus, the fields gone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ask_box_that_grows_near_the_bottom_stays_in_the_page() {
        assert_eq!(ask_edge(100., 230., 600.), Edge::Top(100.));
        // Grown to sixteen lines under the same anchor: on the bottom edge, growing up.
        assert_eq!(ask_edge(100., 520., 600.), Edge::Bottom(4.));
        assert_eq!(ask_edge(500., 230., 600.), Edge::Bottom(4.));
        assert_eq!(ask_edge(100., 598., 600.), Edge::Top(4.));
    }

    #[test]
    fn a_find_boxs_option_keys_are_its_own_and_nothing_else() {
        let key = |s: &str| option_key(&Keystroke::parse(s).unwrap());
        if cfg!(target_os = "macos") {
            assert_eq!(key("alt-cmd-c"), Some('c'));
            assert_eq!(key("alt-cmd-w"), Some('w'));
            assert_eq!(key("alt-cmd-r"), Some('r'));
            assert_eq!(key("alt-c"), None);
            assert_eq!(key("alt-shift-cmd-c"), None);
        } else {
            assert_eq!(key("alt-c"), Some('c'));
            assert_eq!(key("ctrl-alt-c"), None);
        }
        assert_eq!(key("cmd-c"), None);
        assert_eq!(key("alt-cmd-x"), None);
    }
}
