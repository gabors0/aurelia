//! The full-window player: video under Aurelia's own controls.
//!
//! The video itself is a Wayland subsurface below the window
//! (`player::VideoSurface`); this view keeps the area over it transparent
//! and lays the controls on top. It is opaque black until the surface has
//! presented a frame, so the desktop never shows through.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as icon;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnimationExt as _, App, Bounds, ClickEvent, Context, EventEmitter, FocusHandle, Focusable,
    FontWeight, Hsla, KeyBinding, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels,
    SharedString, SpringAnimation, Task, TestSupportExt as _, Window, actions, canvas, div, hsla,
    linear_color_stop, linear_gradient, px,
};
use player::{Control, PlaybackControl, PlayerEvent, Track, TrackChoice, TrackKind, VideoSurface};

use crate::components::key_nav;
use crate::components::motion::{Pressable, SPRING, pressable};
use crate::format;
use crate::playback::{KnownTrack, menu_label};
use crate::theme::{FONT_DISPLAY, Palette};

/// Controls hide after this long without the pointer moving.
pub const HIDE_AFTER: Duration = Duration::from_secs(3);
const SEEK_STEP: f64 = 10.;
const VOLUME_STEP: f64 = 5.;
const CONTEXT: &str = "Player";

actions!(
    player,
    [
        TogglePause,
        SeekBack,
        SeekForward,
        VolumeUp,
        VolumeDown,
        ToggleMute,
        ToggleFullscreen,
        Leave
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("space", TogglePause, Some(CONTEXT)),
        KeyBinding::new("k", TogglePause, Some(CONTEXT)),
        KeyBinding::new("left", SeekBack, Some(CONTEXT)),
        KeyBinding::new("right", SeekForward, Some(CONTEXT)),
        KeyBinding::new("up", VolumeUp, Some(CONTEXT)),
        KeyBinding::new("down", VolumeDown, Some(CONTEXT)),
        KeyBinding::new("m", ToggleMute, Some(CONTEXT)),
        KeyBinding::new("f", ToggleFullscreen, Some(CONTEXT)),
        KeyBinding::new("escape", Leave, Some(CONTEXT)),
    ]);
}

/// What the player shows about the item.
#[derive(Debug, Clone, Default)]
pub struct PlayerInfo {
    /// The movie, or the show.
    pub title: SharedString,
    /// The episode ("S1:E3 · Half Loop"), or the year.
    pub detail: Option<SharedString>,
    pub runtime: Option<Duration>,
    /// The server's audio and subtitle streams, for menu labels.
    pub known_tracks: Vec<KnownTrack>,
    pub volume: player::Volume,
}

pub enum PlayerViewEvent {
    /// The player is done (left by the user, or playback ended).
    Closed,
}

/// Where a click lands along a bar, 0–1.
pub fn fraction_at(x: Pixels, bounds: Bounds<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width);
    if width <= 0. {
        return 0.;
    }
    (f32::from(x - bounds.origin.x) / width).clamp(0., 1.)
}

/// A bar being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    Seek(f32),
    Volume(f32),
}

pub struct PlayerView {
    info: PlayerInfo,
    control: Arc<dyn PlaybackControl>,
    surface: Option<Arc<VideoSurface>>,
    focus: FocusHandle,
    started: bool,
    paused: bool,
    buffering: bool,
    position: Duration,
    duration: Option<Duration>,
    volume: player::Volume,
    tracks: Vec<Track>,
    controls_visible: bool,
    menu_open: bool,
    drag: Option<Drag>,
    seek_bar: Rc<Cell<Bounds<Pixels>>>,
    volume_bar: Rc<Cell<Bounds<Pixels>>>,
    /// Where the top and bottom bars are: clicks there aren't video clicks.
    bars: Rc<Cell<[Bounds<Pixels>; 2]>>,
    /// The window went fullscreen for the player; undo it on leaving.
    entered_fullscreen: bool,
    closed: bool,
    _hide: Option<Task<()>>,
}

impl EventEmitter<PlayerViewEvent> for PlayerView {}

impl Focusable for PlayerView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PlayerView {
    pub fn new(
        info: PlayerInfo,
        control: Arc<dyn PlaybackControl>,
        surface: Option<Arc<VideoSurface>>,
        cx: &mut Context<Self>,
    ) -> Self {
        let volume = info.volume;
        // Gone without being closed (signed out, session expired): stop, and
        // give the window its background back.
        cx.on_release(|this: &mut Self, cx| {
            if !this.closed {
                this.control.stop();
            }
            crate::theme::set_see_through(false, cx);
        })
        .detach();
        let mut this = Self {
            info,
            control,
            surface,
            focus: cx.focus_handle(),
            started: false,
            paused: false,
            buffering: false,
            position: Duration::ZERO,
            duration: None,
            volume,
            tracks: Vec::new(),
            controls_visible: true,
            menu_open: false,
            drag: None,
            seek_bar: Rc::default(),
            volume_bar: Rc::default(),
            bars: Rc::default(),
            entered_fullscreen: false,
            closed: false,
            _hide: None,
        };
        this.activity(cx);
        this
    }

    /// Follows the player.
    pub fn on_event(&mut self, event: &PlayerEvent, cx: &mut Context<Self>) {
        match event {
            PlayerEvent::Started => {
                self.started = true;
                self.activity(cx);
            }
            PlayerEvent::Position(position) | PlayerEvent::Seeked(position) => {
                self.position = *position;
            }
            PlayerEvent::Paused(paused) => {
                self.paused = *paused;
                self.activity(cx);
            }
            PlayerEvent::Duration(duration) => self.duration = Some(*duration),
            PlayerEvent::Buffering(buffering) => self.buffering = *buffering,
            PlayerEvent::Volume(volume) => self.volume = *volume,
            PlayerEvent::Tracks(tracks) => self.tracks = tracks.clone(),
            PlayerEvent::Ended { .. } => {}
        }
        cx.notify();
    }

    /// Leaves the player: stops playback (unless it already ended) and
    /// restores the window.
    pub fn close(&mut self, stop: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.closed {
            return;
        }
        self.closed = true;
        if stop {
            self.control.stop();
        }
        if self.entered_fullscreen && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        cx.emit(PlayerViewEvent::Closed);
    }

    fn length(&self) -> Option<Duration> {
        self.duration.or(self.info.runtime)
    }

    /// The pointer moved or something changed: show the controls, and hide
    /// them again after a while.
    fn activity(&mut self, cx: &mut Context<Self>) {
        if !self.controls_visible {
            self.controls_visible = true;
            cx.notify();
        }
        self._hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(HIDE_AFTER).await;
            this.update(cx, |this, cx| {
                if this.may_hide() {
                    this.controls_visible = false;
                    cx.notify();
                }
            })
            .ok();
        }));
    }

    /// Paused, loading or in use: the controls stay.
    fn may_hide(&self) -> bool {
        self.started && !self.paused && !self.menu_open && self.drag.is_none()
    }

    fn send(&self, control: Control) {
        self.control.send(control);
    }

    fn toggle_pause(&mut self, _: &TogglePause, _: &mut Window, cx: &mut Context<Self>) {
        if self.started {
            self.send(Control::Pause(!self.paused));
        }
        self.activity(cx);
    }

    fn seek_back(&mut self, _: &SeekBack, _: &mut Window, cx: &mut Context<Self>) {
        self.send(Control::SeekBy(-SEEK_STEP));
        self.activity(cx);
    }

    fn seek_forward(&mut self, _: &SeekForward, _: &mut Window, cx: &mut Context<Self>) {
        self.send(Control::SeekBy(SEEK_STEP));
        self.activity(cx);
    }

    fn set_volume(&mut self, level: f64, cx: &mut Context<Self>) {
        let level = level.clamp(0., 100.);
        self.send(Control::Volume(level));
        if self.volume.muted && level > 0. {
            self.send(Control::Mute(false));
        }
        self.activity(cx);
    }

    fn volume_up(&mut self, _: &VolumeUp, _: &mut Window, cx: &mut Context<Self>) {
        self.set_volume(self.volume.level + VOLUME_STEP, cx);
    }

    fn volume_down(&mut self, _: &VolumeDown, _: &mut Window, cx: &mut Context<Self>) {
        self.set_volume(self.volume.level - VOLUME_STEP, cx);
    }

    fn toggle_mute(&mut self, _: &ToggleMute, _: &mut Window, cx: &mut Context<Self>) {
        self.send(Control::Mute(!self.volume.muted));
        self.activity(cx);
    }

    fn toggle_fullscreen(
        &mut self,
        _: &ToggleFullscreen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !window.is_fullscreen() {
            self.entered_fullscreen = true;
        }
        window.toggle_fullscreen();
        self.activity(cx);
    }

    fn leave(&mut self, _: &Leave, window: &mut Window, cx: &mut Context<Self>) {
        if window.is_fullscreen() {
            window.toggle_fullscreen();
            self.activity(cx);
        } else {
            self.close(true, window, cx);
        }
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        match self.drag {
            Some(_) if event.pressed_button != Some(MouseButton::Left) => self.end_drag(cx),
            Some(Drag::Seek(_)) => {
                self.drag = Some(Drag::Seek(fraction_at(
                    event.position.x,
                    self.seek_bar.get(),
                )));
                cx.notify();
            }
            Some(Drag::Volume(_)) => {
                let fraction = fraction_at(event.position.x, self.volume_bar.get());
                self.drag = Some(Drag::Volume(fraction));
                self.set_volume(f64::from(fraction) * 100., cx);
            }
            None => {}
        }
        self.activity(cx);
    }

    fn start_drag(&mut self, drag: Drag, cx: &mut Context<Self>) {
        self.drag = Some(drag);
        if let Drag::Volume(fraction) = drag {
            self.set_volume(f64::from(fraction) * 100., cx);
        }
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if let Some(Drag::Seek(fraction)) = self.drag.take()
            && let Some(length) = self.length()
        {
            let target = length.mul_f32(fraction);
            self.position = target;
            self.send(Control::Seek(target));
        }
        self.activity(cx);
        cx.notify();
    }

    /// A click on the video toggles pause, a double click fullscreen. On a
    /// bar it's the bar's (its buttons see it too); with the controls hidden,
    /// it only brings them back.
    fn click_video(&mut self, event: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let on_bar = self
            .bars
            .get()
            .iter()
            .any(|bar| bar.contains(&event.position()));
        if on_bar {
            self.activity(cx);
        } else if event.click_count() >= 2 {
            self.toggle_fullscreen(&ToggleFullscreen, window, cx);
        } else {
            self.toggle_pause(&TogglePause, window, cx);
        }
    }

    fn render_top(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let info = &self.info;
        let bars = self.bars.clone();
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(140.))
            .child(
                canvas(
                    move |b, _, _| {
                        let [_, bottom] = bars.get();
                        // Only the row of controls, not the whole fade.
                        bars.set([
                            Bounds::new(b.origin, gpui_kit::size(b.size.width, px(84.))),
                            bottom,
                        ]);
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .bg(linear_gradient(
                180.,
                linear_color_stop(hsla(0., 0., 0., 0.7), 0.),
                linear_color_stop(hsla(0., 0., 0., 0.), 1.),
            ))
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .px_6()
                    .pt_5()
                    .child(
                        control_button("player-back", icon::ArrowLeft, 40.)
                            .tooltip(|window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new("Back (Esc)")
                                    .build(window, cx)
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.close(true, window, cx)),
                            ),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .pt(px(4.))
                            .child(
                                div()
                                    .font_family(FONT_DISPLAY)
                                    .font_weight(FontWeight::BOLD)
                                    .text_size(px(22.))
                                    .text_color(on_video())
                                    .truncate()
                                    .child(info.title.clone()),
                            )
                            .when_some(info.detail.clone(), |this, detail| {
                                this.child(
                                    div().text_color(on_video_soft()).truncate().child(detail),
                                )
                            }),
                    ),
            )
    }

    fn render_seek_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let length = self.length();
        let played = match (self.drag, length) {
            (Some(Drag::Seek(fraction)), _) => fraction,
            (_, Some(length)) if !length.is_zero() => {
                (self.position.as_secs_f32() / length.as_secs_f32()).clamp(0., 1.)
            }
            _ => 0.,
        };
        let accent: Hsla = Palette::accent().into();
        let bounds = self.seek_bar.clone();
        div()
            .id("seek-bar")
            .relative()
            .w_full()
            .h(px(20.))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let fraction = fraction_at(event.position.x, this.seek_bar.get());
                    this.start_drag(Drag::Seek(fraction), cx);
                }),
            )
            .child(
                canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                    .absolute()
                    .inset_0(),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px(4.))
                    .rounded_full()
                    .bg(hsla(0., 0., 1., 0.25))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .bottom_0()
                            .w(gpui_kit::relative(played))
                            .rounded_full()
                            .bg(accent),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(-5.))
                            .left(gpui_kit::relative(played))
                            .ml(px(-7.))
                            .size(px(14.))
                            .rounded_full()
                            .bg(gpui_kit::white()),
                    ),
            )
            .test_support()
    }

    fn render_volume(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let volume = self.volume;
        let level = match self.drag {
            Some(Drag::Volume(fraction)) => fraction,
            _ if volume.muted => 0.,
            _ => (volume.level / 100.).clamp(0., 1.) as f32,
        };
        let glyph = if volume.muted || volume.level <= 0. {
            icon::VolumeX
        } else if volume.level < 50. {
            icon::Volume1
        } else {
            icon::Volume2
        };
        let bounds = self.volume_bar.clone();
        h_flex()
            .gap_2()
            .child(
                control_button("player-mute", glyph, 40.)
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Mute (M)").build(window, cx)
                    })
                    .on_click(
                        cx.listener(|this, _, window, cx| {
                            this.toggle_mute(&ToggleMute, window, cx)
                        }),
                    ),
            )
            .child(
                div()
                    .id("volume-bar")
                    .relative()
                    .w(px(96.))
                    .h(px(20.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            cx.stop_propagation();
                            let fraction = fraction_at(event.position.x, this.volume_bar.get());
                            this.start_drag(Drag::Volume(fraction), cx);
                        }),
                    )
                    .child(
                        canvas(move |b, _, _| bounds.set(b), |_, _, _, _| {})
                            .absolute()
                            .inset_0(),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(4.))
                            .rounded_full()
                            .bg(hsla(0., 0., 1., 0.25))
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(gpui_kit::relative(level))
                                    .rounded_full()
                                    .bg(gpui_kit::white()),
                            ),
                    ),
            )
    }

    fn render_track_menu(&self, kind: TrackKind, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, glyph, tooltip) = match kind {
            TrackKind::Subtitle => ("player-subtitles", icon::Captions, "Subtitles"),
            _ => ("player-audio", icon::AudioLines, "Audio"),
        };
        let entries: Vec<(SharedString, u32, bool)> = self
            .tracks
            .iter()
            .filter(|t| t.kind == Some(kind))
            .map(|t| {
                (
                    menu_label(t, &self.info.known_tracks).into(),
                    t.id,
                    t.selected,
                )
            })
            .collect();
        let none_selected = !entries.iter().any(|(_, _, selected)| *selected);
        let control = self.control.clone();
        let view = cx.weak_entity();
        control_button(id, glyph, 40.)
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx)
            })
            .dropdown_menu_with_anchor(gpui_kit::Anchor::BottomRight, move |menu, _, _| {
                let mut menu = menu;
                if kind == TrackKind::Subtitle {
                    let control = control.clone();
                    menu = menu.item(PopupMenuItem::new("Off").checked(none_selected).on_click(
                        move |_, _, _| control.send(Control::Subtitle(TrackChoice::Off)),
                    ));
                }
                for (label, track, selected) in &entries {
                    let control = control.clone();
                    let track = *track;
                    menu = menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(*selected)
                            .on_click(move |_, _, _| {
                                control.send(match kind {
                                    TrackKind::Subtitle => {
                                        Control::Subtitle(TrackChoice::Id(track))
                                    }
                                    _ => Control::Audio(TrackChoice::Id(track)),
                                })
                            }),
                    );
                }
                menu
            })
            .on_open_change(move |open, _, cx| {
                let open = *open;
                view.update(cx, |this, cx| {
                    this.menu_open = open;
                    this.activity(cx);
                })
                .ok();
            })
    }

    fn render_bottom(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let length = self.length();
        let shown = match (self.drag, length) {
            (Some(Drag::Seek(fraction)), Some(length)) => length.mul_f32(fraction),
            _ => self.position,
        };
        let times = match length {
            Some(length) => format!("{} / {}", format::clock(shown), format::clock(length)),
            None => format::clock(shown),
        };
        let has_audio_choice = self
            .tracks
            .iter()
            .filter(|t| t.kind == Some(TrackKind::Audio))
            .count()
            > 1;
        let has_subtitles = self
            .tracks
            .iter()
            .any(|t| t.kind == Some(TrackKind::Subtitle));
        let fullscreen = window.is_fullscreen();
        let bars = self.bars.clone();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .pt(px(80.))
            .bg(linear_gradient(
                0.,
                linear_color_stop(hsla(0., 0., 0., 0.75), 0.),
                linear_color_stop(hsla(0., 0., 0., 0.), 1.),
            ))
            .child(
                v_flex()
                    .relative()
                    .child(
                        canvas(
                            move |b, _, _| {
                                let [top, _] = bars.get();
                                bars.set([top, b]);
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .inset_0(),
                    )
                    .px_6()
                    .pb_5()
                    .gap_2()
                    .child(self.render_seek_bar(cx))
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        control_button(
                                            "player-play",
                                            if self.paused { icon::Play } else { icon::Pause },
                                            48.,
                                        )
                                        .tooltip(|window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(
                                                "Play / pause (Space)",
                                            )
                                            .build(window, cx)
                                        })
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.toggle_pause(&TogglePause, window, cx)
                                            }),
                                        ),
                                    )
                                    .child(
                                        control_button("player-back10", icon::RotateCcw, 40.)
                                            .tooltip(|window, cx| {
                                                gpui_kit::component::tooltip::Tooltip::new(
                                                    "Back 10 seconds (←)",
                                                )
                                                .build(window, cx)
                                            })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.seek_back(&SeekBack, window, cx)
                                            })),
                                    )
                                    .child(
                                        control_button("player-forward10", icon::RotateCw, 40.)
                                            .tooltip(|window, cx| {
                                                gpui_kit::component::tooltip::Tooltip::new(
                                                    "Forward 10 seconds (→)",
                                                )
                                                .build(window, cx)
                                            })
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.seek_forward(&SeekForward, window, cx)
                                            })),
                                    )
                                    .child(self.render_volume(cx))
                                    .child(
                                        div()
                                            .ml_3()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(on_video_soft())
                                            .child(times),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .when(has_audio_choice, |this| {
                                        this.child(self.render_track_menu(TrackKind::Audio, cx))
                                    })
                                    .when(has_subtitles, |this| {
                                        this.child(self.render_track_menu(TrackKind::Subtitle, cx))
                                    })
                                    .child(
                                        control_button(
                                            "player-fullscreen",
                                            if fullscreen {
                                                icon::Minimize
                                            } else {
                                                icon::Maximize
                                            },
                                            40.,
                                        )
                                        .tooltip(|window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(
                                                "Full screen (F)",
                                            )
                                            .build(window, cx)
                                        })
                                        .on_click(
                                            cx.listener(|this, _, window, cx| {
                                                this.toggle_fullscreen(
                                                    &ToggleFullscreen,
                                                    window,
                                                    cx,
                                                )
                                            }),
                                        ),
                                    ),
                            ),
                    ),
            )
    }
}

/// Text and glyphs over video: white whatever the theme (like artwork).
fn on_video() -> Hsla {
    hsla(0., 0., 1., 1.)
}

fn on_video_soft() -> Hsla {
    hsla(0., 0., 1., 0.75)
}

/// A round glyph button over video. Mouse only: the keys have their own
/// bindings, and Space must not both press a button and pause.
fn control_button(id: &'static str, glyph: icon, size: f32) -> Pressable {
    pressable(id).unfocusable().look(move |this, m| {
        this.relative()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .bg(hsla(0., 0., 1., 0.16 * m.amount()))
            .child(Icon::new(glyph).size(px(size * 0.5)).text_color(on_video()))
    })
}

impl Render for PlayerView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ready = self.surface.as_ref().is_none_or(|s| s.is_ready());
        if self.surface.is_some() {
            if ready {
                if !crate::theme::see_through() {
                    // Not from inside render: it changes a global.
                    cx.defer(|cx| crate::theme::set_see_through(true, cx));
                }
            } else {
                // Look again next frame.
                window.request_animation_frame();
            }
        }
        let surface = self.surface.clone();
        let loading = !self.started || self.buffering;
        let visible = self.controls_visible || !self.may_hide();

        div()
            .id("player")
            .relative()
            .size_full()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .when(!ready || surface.is_none(), |this| {
                this.bg(gpui_kit::black())
            })
            .on_action(cx.listener(Self::toggle_pause))
            .on_action(cx.listener(Self::seek_back))
            .on_action(cx.listener(Self::seek_forward))
            .on_action(cx.listener(Self::volume_up))
            .on_action(cx.listener(Self::volume_down))
            .on_action(cx.listener(Self::toggle_mute))
            .on_action(cx.listener(Self::toggle_fullscreen))
            .on_action(cx.listener(Self::leave))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.drag.is_some() {
                        this.end_drag(cx);
                    }
                }),
            )
            // Keys fall back here while the player is shown.
            .child(key_nav::scope(self.focus.clone(), None, px(0.)))
            .child(
                canvas(
                    move |bounds, window, _| {
                        if let Some(surface) = &surface {
                            surface.place(
                                f32::from(bounds.origin.x),
                                f32::from(bounds.origin.y),
                                f32::from(bounds.size.width),
                                f32::from(bounds.size.height),
                                window.scale_factor(),
                            );
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .child(
                div()
                    .id("player-video")
                    .absolute()
                    .inset_0()
                    .on_click(cx.listener(Self::click_video)),
            )
            .when(loading, |this| {
                this.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Spinner::new().large().color(on_video())),
                )
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .child(self.render_top(cx))
                    .child(self.render_bottom(window, cx))
                    .with_spring(
                        "player-controls",
                        SpringAnimation::new(SPRING).to(visible),
                        |this, phase| this.opacity(phase.0.clamp(0., 1.)),
                    ),
            )
            // Hidden controls take no clicks: this covers them (and the
            // video), and the first move or click brings them back.
            .when(!visible, |this| {
                this.child(
                    div()
                        .id("player-wake")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_move(cx.listener(|this, _, _, cx| this.activity(cx)))
                        .on_click(cx.listener(Self::click_video)),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AnyWindowHandle, Entity, TestAppContext, WindowBounds, WindowOptions, point, size,
    };
    use player::Volume;

    /// Records what the view asks of the player.
    #[derive(Default)]
    struct FakeControl {
        sent: Mutex<Vec<Control>>,
        stopped: AtomicBool,
    }

    impl PlaybackControl for FakeControl {
        fn stop(&self) {
            self.stopped.store(true, Ordering::SeqCst);
        }

        fn send(&self, control: Control) {
            self.sent.lock().unwrap().push(control);
        }
    }

    impl FakeControl {
        fn take(&self) -> Vec<Control> {
            std::mem::take(&mut self.sent.lock().unwrap())
        }
    }

    /// A playing view (100 s long, volume 50) in an 800×450 window.
    fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<PlayerView>, Arc<FakeControl>) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let control = Arc::new(FakeControl::default());
        let info = PlayerInfo {
            title: "Dune".into(),
            detail: Some("2024".into()),
            runtime: Some(Duration::from_secs(100)),
            known_tracks: Vec::new(),
            volume: Volume {
                level: 50.,
                muted: false,
            },
        };
        let mut view = None;
        let (window, _) = cx.update(|cx| {
            let control = control.clone() as Arc<dyn PlaybackControl>;
            let view = &mut view;
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        Default::default(),
                        size(px(800.), px(450.)),
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    let player = cx.new(|cx| PlayerView::new(info, control, None, cx));
                    window.focus(&player.focus_handle(cx), cx);
                    *view = Some(player.clone());
                    player
                },
            )
            .expect("open test window")
        });
        let view = view.unwrap();
        cx.update(|cx| {
            view.update(cx, |view, cx| {
                view.on_event(&PlayerEvent::Duration(Duration::from_secs(100)), cx);
                view.on_event(&PlayerEvent::Started, cx);
            })
        });
        (window, view, control)
    }

    #[gpui_kit::test]
    fn keys_drive_the_player(cx: &mut TestAppContext) {
        let (window, _, control) = open(cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            for key in ["space", "right", "left", "up", "down", "m"] {
                window.press(key, cx);
            }
        })
        .unwrap();
        assert_eq!(
            control.take(),
            vec![
                Control::Pause(true),
                Control::SeekBy(10.),
                Control::SeekBy(-10.),
                Control::Volume(55.),
                Control::Volume(45.),
                Control::Mute(true),
            ]
        );
    }

    #[gpui_kit::test]
    fn escape_leaves_and_stops(cx: &mut TestAppContext) {
        let (window, view, control) = open(cx);
        let closed = Rc::new(Cell::new(false));
        cx.update(|cx| {
            let closed = closed.clone();
            cx.subscribe(&view, move |_, PlayerViewEvent::Closed, _| closed.set(true))
                .detach();
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        cx.run_until_parked();
        assert!(control.stopped.load(Ordering::SeqCst), "playback stops");
        assert!(closed.get(), "the shell is told");
    }

    #[gpui_kit::test]
    fn clicking_the_seek_bar_seeks_there(cx: &mut TestAppContext) {
        let (window, _, control) = open(cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            // The middle of the bar: half way through.
            window.click("seek-bar", cx);
        })
        .unwrap();
        let sent = control.take();
        assert_eq!(sent, vec![Control::Seek(Duration::from_secs(50))]);
    }

    #[gpui_kit::test]
    fn dropping_the_view_stops_playback(cx: &mut TestAppContext) {
        let (window, view, control) = open(cx);
        drop(view);
        cx.update(|cx| window.update(cx, |_, window, _| window.remove_window()))
            .unwrap();
        cx.run_until_parked();
        assert!(control.stopped.load(Ordering::SeqCst));
        assert!(!crate::theme::see_through());
    }

    #[gpui_kit::test]
    fn controls_hide_while_playing_but_not_while_paused(cx: &mut TestAppContext) {
        let (_, view, _) = open(cx);
        let visible = |cx: &mut TestAppContext| view.read_with(cx, |v, _| v.controls_visible);
        assert!(visible(cx));
        cx.executor()
            .advance_clock(HIDE_AFTER + Duration::from_millis(100));
        cx.run_until_parked();
        assert!(!visible(cx), "hidden after a quiet while");

        cx.update(|cx| view.update(cx, |v, cx| v.on_event(&PlayerEvent::Paused(true), cx)));
        assert!(visible(cx), "pausing shows them");
        cx.executor().advance_clock(HIDE_AFTER * 2);
        cx.run_until_parked();
        assert!(visible(cx), "and they stay while paused");
    }

    #[test]
    fn clicks_map_to_fractions_of_the_bar() {
        let bar = Bounds::new(point(px(100.), px(0.)), size(px(200.), px(20.)));
        assert_eq!(fraction_at(px(100.), bar), 0.);
        assert_eq!(fraction_at(px(150.), bar), 0.25);
        assert_eq!(fraction_at(px(300.), bar), 1.);
        assert_eq!(fraction_at(px(20.), bar), 0., "clamped before the bar");
        assert_eq!(fraction_at(px(900.), bar), 1., "clamped after the bar");
        let empty = Bounds::new(point(px(0.), px(0.)), size(px(0.), px(0.)));
        assert_eq!(fraction_at(px(5.), empty), 0.);
    }
}
