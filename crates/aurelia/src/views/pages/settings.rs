//! Settings: theme, motion, the Home slideshow, accounts on this device, and
//! the keyboard shortcuts.

use gpui_kit::AnimationExt as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, ElementId, FontWeight, Hsla, ScrollHandle, SharedString, SpringAnimation,
    Window, div, px,
};

use crate::components::button::{focus_ring, glass_button};
use crate::components::motion::{SPRING, mix, pressable};
use crate::components::pill::pill;
use crate::components::row::ROW_PADDING;
use crate::session::Session;
use crate::settings::{self, Settings, ThemeChoice};
use crate::state::AppState;
use crate::theme::{FONT_DISPLAY, Palette};
use crate::views::shell::{self, NAV_HEIGHT};

const THEMES: [(ThemeChoice, &str); 3] = [
    (ThemeChoice::Dark, "Dark"),
    (ThemeChoice::Light, "Light"),
    (ThemeChoice::System, "Match system"),
];

const SHORTCUTS: [(&str, &str); 10] = [
    ("↑ ↓ ← →", "Move between posters, buttons and shelves"),
    (
        "Enter / Space",
        "Open or press what's focused; with nothing focused, play the page",
    ),
    ("Tab / Shift+Tab", "Next / previous control, menus included"),
    (
        "← / →",
        "Previous / next slide on Home, before anything is focused",
    ),
    ("Esc, Alt+←", "Back"),
    ("Alt+→", "Forward"),
    ("Ctrl+F", "Search"),
    ("Ctrl+R, F5", "Refresh"),
    ("Ctrl+,", "Settings"),
    ("Ctrl+Q", "Quit"),
];

pub struct SettingsPage {
    scroll: ScrollHandle,
    /// Saved accounts, read when the page opens or refreshes.
    accounts: Vec<Session>,
}

impl SettingsPage {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            scroll: ScrollHandle::new(),
            accounts: Vec::new(),
        };
        this.refresh(cx);
        this
    }

    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.accounts = AppState::global(cx).store().accounts();
        cx.notify();
    }

    fn change(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut Settings)) {
        settings::update(cx, change);
        cx.notify();
    }

    fn remove(&mut self, account: Session, cx: &mut Context<Self>) {
        crate::accounts::remove(&account, cx);
        self.refresh(cx);
    }

    fn render_theme(&self, current: ThemeChoice, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .gap_2()
            .children(THEMES.iter().map(|(choice, label)| {
                let choice = *choice;
                pill(
                    SharedString::from(format!("theme-{label}")),
                    *label,
                    current == choice,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.change(cx, |s| s.theme = choice);
                }))
            }))
            .into_any_element()
    }

    fn render_accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = AppState::global(cx);
        let current = state.session().cloned();
        let rows: Vec<AnyElement> = self
            .accounts
            .iter()
            .map(|account| {
                let is_current = current.as_ref().is_some_and(|c| c.is_same_account(account));
                let id =
                    SharedString::from(format!("account-{}-{}", account.server, account.user_id));
                let to_remove = account.clone();
                h_flex()
                    .gap_4()
                    .py_2()
                    .child(crate::views::profiles::avatar(
                        id.clone(),
                        account,
                        state,
                        px(40.),
                    ))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(account.user_name.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(Palette::text_tertiary())
                                    .truncate()
                                    .child(format!("{} · {}", account.server_name, account.server)),
                            ),
                    )
                    .child(if is_current {
                        div()
                            .text_sm()
                            .text_color(Palette::text_secondary())
                            .child("Signed in")
                            .into_any_element()
                    } else {
                        small_button(SharedString::from(format!("{id}-remove")), "Remove")
                            .on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.remove(to_remove.clone(), cx)
                                }),
                            )
                            .into_any_element()
                    })
                    .into_any_element()
            })
            .collect();
        v_flex()
            .gap_1()
            .children(rows)
            .child(
                h_flex()
                    .gap_3()
                    .pt_3()
                    .child(
                        glass_button("switch-user", Some(IconName::Users), "Switch user…")
                            .on_click(|_, _, cx| shell::switch_user(cx)),
                    )
                    .child(
                        glass_button("add-account", Some(IconName::UserPlus), "Add account")
                            .on_click(|_, _, cx| shell::add_account(cx)),
                    ),
            )
            .into_any_element()
    }
}

/// A compact glass button for inline actions.
fn small_button(id: SharedString, label: &'static str) -> crate::components::motion::Pressable {
    pressable(id).look(move |this, m| {
        this.relative()
            .h(px(32.))
            .px_4()
            .flex()
            .items_center()
            .rounded_full()
            .cursor_pointer()
            .text_sm()
            .font_weight(FontWeight::MEDIUM)
            .border_1()
            .border_color(Palette::border())
            .bg(m.mix(Palette::glass(), Palette::glass_hover()))
            .child(focus_ring(m, px(16.)))
            .child(label)
    })
}

/// A setting that's on or off: the whole row toggles it.
fn switch_row(
    id: &'static str,
    title: &'static str,
    detail: &'static str,
    on: bool,
    toggle: impl Fn(&mut Window, &mut gpui_kit::App) + 'static,
) -> AnyElement {
    let accent: Hsla = Palette::accent().into();
    pressable(id)
        .on_click(move |_, window, cx| toggle(window, cx))
        .look(move |this, m| {
            this.relative()
                .flex()
                .items_center()
                .gap_6()
                .px_4()
                .py_3()
                .mx(px(-16.))
                .rounded(px(14.))
                .cursor_pointer()
                .bg(mix(
                    gpui_kit::transparent_black(),
                    Palette::glass().into(),
                    m.amount(),
                ))
                .child(focus_ring(m, px(14.)))
                .child(
                    v_flex()
                        .flex_1()
                        .gap_0p5()
                        .child(div().font_weight(FontWeight::MEDIUM).child(title))
                        .child(
                            div()
                                .text_sm()
                                .text_color(Palette::text_secondary())
                                .child(detail),
                        ),
                )
                .child(switch(
                    ElementId::from((ElementId::from(id), "switch")),
                    on,
                    accent,
                ))
        })
        .into_any_element()
}

/// The switch itself; its knob slides.
fn switch(id: ElementId, on: bool, accent: Hsla) -> impl IntoElement {
    let off_track: Hsla = Palette::glass_hover().into();
    div()
        .w(px(44.))
        .h(px(26.))
        .flex_shrink_0()
        .rounded_full()
        .p(px(3.))
        .child(div().size(px(20.)).rounded_full().bg(gpui_kit::white()))
        .with_spring(
            id,
            SpringAnimation::new(SPRING).to(on),
            move |this, phase| {
                let t = phase.0;
                this.bg(mix(off_track, accent, t.clamp(0., 1.)))
                    .pl(px(3. + 18. * t.clamp(-0.1, 1.1)))
            },
        )
}

fn section(title: &'static str, children: Vec<AnyElement>) -> AnyElement {
    v_flex()
        .gap_3()
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::BOLD)
                .text_color(Palette::text_tertiary())
                .child(title.to_uppercase()),
        )
        .child(
            v_flex()
                .gap_1()
                .p_5()
                .rounded(px(18.))
                .bg(Palette::glass())
                .border_1()
                .border_color(Palette::border())
                .children(children),
        )
        .into_any_element()
}

fn labelled(label: &'static str, control: AnyElement) -> AnyElement {
    h_flex()
        .justify_between()
        .gap_6()
        .py_1()
        .child(div().font_weight(FontWeight::MEDIUM).child(label))
        .child(control)
        .into_any_element()
}

fn keycap(keys: &'static str) -> AnyElement {
    div()
        .w(px(150.))
        .flex_shrink_0()
        .child(
            div().flex().child(
                div()
                    .px_2()
                    .py(px(2.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(Palette::border())
                    .bg(Palette::glass())
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(keys),
            ),
        )
        .into_any_element()
}

impl Render for SettingsPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = settings::get(cx).clone();
        let session = AppState::global(cx).session().cloned();
        let signed_in = session
            .map(|s| format!("Signed in as {} on {}", s.user_name, s.server_name))
            .unwrap_or_default();
        let page = cx.weak_entity();
        let toggle = move |change: fn(&mut Settings)| {
            let page = page.clone();
            move |_: &mut Window, cx: &mut gpui_kit::App| {
                page.update(cx, |page, cx| page.change(cx, change)).ok();
            }
        };

        let sections = vec![
            section(
                "Appearance",
                vec![labelled("Theme", self.render_theme(current.theme, cx))],
            ),
            section(
                "Motion",
                vec![switch_row(
                    "animations",
                    "Animations",
                    "Page transitions, cards that lift as you point at them, and smooth state changes.",
                    current.animations,
                    toggle(|s| s.animations = !s.animations),
                )],
            ),
            section(
                "Home",
                vec![switch_row(
                    "hero-autoplay",
                    "Play the slideshow",
                    "Move to the next slide every 8 seconds, unless you're pointing at it.",
                    current.hero_autoplay,
                    toggle(|s| s.hero_autoplay = !s.hero_autoplay),
                )],
            ),
            section(
                "Accounts",
                vec![
                    switch_row(
                        "profile-picker",
                        "Ask who's watching",
                        "At launch, when more than one account is saved on this device.",
                        current.profile_picker,
                        toggle(|s| s.profile_picker = !s.profile_picker),
                    ),
                    div().h_2().into_any_element(),
                    self.render_accounts(cx),
                ],
            ),
            section(
                "Keyboard",
                SHORTCUTS
                    .iter()
                    .map(|(keys, action)| {
                        h_flex()
                            .gap_4()
                            .py_1()
                            .child(keycap(keys))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(Palette::text_secondary())
                                    .child(*action),
                            )
                            .into_any_element()
                    })
                    .collect(),
            ),
        ];

        div()
            .relative()
            .size_full()
            .bg(Palette::bg())
            .child(
                crate::components::scroller::page("settings-scroll", &self.scroll).child(
                    v_flex()
                        .px(ROW_PADDING)
                        .pt(NAV_HEIGHT + px(28.))
                        .pb_16()
                        .child(
                            v_flex()
                                .max_w(px(760.))
                                .gap_8()
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .font_family(FONT_DISPLAY)
                                                .font_weight(FontWeight::EXTRA_BOLD)
                                                .text_size(px(44.))
                                                .line_height(px(48.))
                                                .child("Settings"),
                                        )
                                        .child(div().text_color(Palette::text_tertiary()).child(
                                            format!(
                                                "Aurelia {} · {signed_in}",
                                                env!("CARGO_PKG_VERSION")
                                            ),
                                        )),
                                )
                                .children(sections),
                        ),
                ),
            )
            .vertical_scrollbar(&self.scroll)
    }
}
