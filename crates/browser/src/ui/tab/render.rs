//! The tab's bars and its page area: address and tools, device, find, downloads, console, and
//! the area the page's view lies over (or its picture, a placeholder, a device's frame).

use super::*;
use gitviber_ui::Settings;
use gpui::{MouseButton, ObjectFit, StyledImage as _};
use gpui_kit::component::checkbox::Checkbox;

/// The page's px from its viewport's top left, in the tab's px: where a pick's mark goes.
struct Mark {
    left: f32,
    top: f32,
    w: f32,
    h: f32,
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
        let title = self
            .state(cx)
            .map(|s| s.title.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| self.saved.title.clone().filter(|t| !t.is_empty()));
        title
            .unwrap_or_else(|| page_label(&self.current_url(cx)))
            .into()
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
        let ports = self.ports.clone();
        let ports_menu = icon_button("ports", IconName::Network, "Ports the terminals serve")
            .dropdown_menu(move |mut menu, _, _| {
                let list = ports.borrow().clone();
                menu = menu.label(if list.is_empty() {
                    "No terminal here serves a port"
                } else {
                    "Served by this worktree's terminals"
                });
                for p in list {
                    let weak = weak.clone();
                    let url = url::port_url(p.port);
                    menu = menu.item(
                        PopupMenuItem::new(format!("localhost:{}  {}", p.port, p.process))
                            .on_click(move |_, window, cx| {
                                let _ = weak.update(cx, |this, cx| this.open_url(&url, window, cx));
                            }),
                    );
                }
                menu
            })
            .on_open_change({
                let weak = cx.weak_entity();
                move |open, window, cx| {
                    if *open {
                        let _ = weak.update(cx, |this, cx| this.read_ports(window, cx));
                    }
                }
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
                        "Stop picking (Esc)"
                    } else {
                        "Pick an element for the agent (⇧⌘C)"
                    },
                )
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
                        "Leave device mode (⇧⌘M)"
                    } else {
                        "Device mode (⇧⌘M)"
                    },
                )
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

    fn device_bar(&mut self, t: &AppTheme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (device, choice) = self.device()?;
        let area = self.area.get();
        let fitted = devices::fit(
            &device,
            choice.rotated,
            (f64::from(area.0), f64::from(area.1)),
        );
        let responsive = choice.name == devices::RESPONSIVE;
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
                            let _ = weak.update(cx, |this, cx| {
                                this.choose_device(Some(next), window, cx)
                            });
                        },
                    ));
                }
                menu.separator().label(
                    "Not quite the device: no touch events beyond touch.js, <meta name=viewport> is ignored, hover still works.",
                )
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
                        .selected(f.match_case)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(f) = this.find.as_mut() {
                                f.match_case = !f.match_case;
                            }
                            this.run_find(false, cx);
                        })),
                )
                .child(
                    icon_button("find-prev", IconName::ChevronUp, "Previous (⇧⌘G)")
                        .on_click(cx.listener(|this, _, _, cx| this.run_find(true, cx))),
                )
                .child(
                    icon_button("find-next", IconName::ChevronDown, "Next (⌘G)")
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
            let text = match d.state.as_str() {
                "going" if d.total > 0 => format!(
                    "{}% of {}",
                    (d.received * 100 / d.total).min(100),
                    format::format_bytes(d.total)
                ),
                "going" if d.received > 0 => format::format_bytes(d.received),
                "going" => "Starting".into(),
                "done" => format!(
                    "Saved to {}",
                    d.path
                        .as_deref()
                        .and_then(|p| std::path::Path::new(p).parent()?.file_name())
                        .map_or("Downloads".into(), |n| n.to_string_lossy().into_owned())
                ),
                "cancelled" => "Cancelled".into(),
                _ => d
                    .why
                    .as_deref()
                    .map_or("Failed".into(), |w| format!("Failed: {w}")),
            };
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

    fn marks(&self, t: &AppTheme) -> Vec<AnyElement> {
        let Some(picks) = &self.picks else {
            return Vec::new();
        };
        let factor = self.factor() as f32;
        let area = self.area.get();
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
        let (w, h) = (320., 150.);
        let left = ask.at.0.clamp(4., (area.0 - w - 4.).max(4.));
        let top = if ask.at.1 + h > area.1 {
            (area.1 - h - 4.).max(4.)
        } else {
            ask.at.1
        };
        let errors = self.page_info(cx).map_or(0, |p| p.errors);
        let picks = self.picks.is_some();
        let include = ask.include_errors;
        Some(
            v_flex()
                .id("ask")
                .absolute()
                .left(px(left))
                .top(px(top))
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
                    div()
                        .truncate()
                        .font_family(t.fonts.code.clone())
                        .text_size(px(11.))
                        .text_color(t.muted_foreground)
                        .child(ask.about.summary.clone()),
                )
                .child(Input::new(&ask.input).xsmall())
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
                        .justify_end()
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
                                .label("Ask ↵")
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
        let area = div()
            .id("browser-page")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(t.background)
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
        let Some((device, choice)) = self.device() else {
            return area
                .child(self.page_canvas(None, live, cx))
                .children(self.picture())
                .children(self.marks(t))
                .children(self.ask_box(t, cx))
                .into_any_element();
        };
        let size = self.area.get();
        let fitted = devices::fit(
            &device,
            choice.rotated,
            (f64::from(size.0), f64::from(size.1)),
        );
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
        let s = f.screen;
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
        let frame = f.frame;
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
            .children(self.device_bar(&t, cx))
            .children(self.find_bar(&t, cx))
            .children(self.download_bar(&t, cx))
            .children(self.console_panel(&t, cx))
            .child(self.page_area(&t, cx))
    }
}
