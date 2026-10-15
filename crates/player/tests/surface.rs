//! `VideoSurface` under a real Wayland window: needs a compositor
//! (`WAYLAND_DISPLAY`, e.g. headless sway), libEGL, libmpv and `mpv`.

mod common;

use std::ptr::NonNull;
use std::time::{Duration, Instant};

use player::{EmbeddedPlayer, EndReason, PlayRequest, PlayerEvent, VideoSurface};
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_registry::WlRegistry, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

#[derive(Default)]
struct App {
    configured: bool,
}

impl Dispatch<WlRegistry, GlobalListContents> for App {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(App: WlCompositor);
delegate_noop!(App: ignore WlSurface);
delegate_noop!(App: ignore xdg_toplevel::XdgToplevel);

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for App {
    fn event(
        _: &mut Self,
        base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for App {
    fn event(
        app: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
            app.configured = true;
        }
    }
}

/// The raw handles a toolkit window would give.
struct Window {
    display: NonNull<std::ffi::c_void>,
    surface: NonNull<std::ffi::c_void>,
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(self.display));
        // SAFETY: the connection outlives the test.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(self.surface));
        // SAFETY: the surface outlives the test.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

#[test]
#[ignore = "needs a Wayland compositor, EGL and libmpv"]
fn video_plays_in_a_subsurface() {
    let connection = Connection::connect_to_env().expect("WAYLAND_DISPLAY");
    let (globals, mut queue) = registry_queue_init::<App>(&connection).unwrap();
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 4..=6, ()).unwrap();
    let base: xdg_wm_base::XdgWmBase = globals.bind(&qh, 1..=1, ()).unwrap();
    let surface = compositor.create_surface(&qh, ());
    let xdg = base.get_xdg_surface(&surface, &qh, ());
    let toplevel = xdg.get_toplevel(&qh, ());
    toplevel.set_title("aurelia surface test".into());
    surface.commit();
    let mut app = App::default();
    while !app.configured {
        queue.blocking_dispatch(&mut app).unwrap();
    }
    surface.commit();

    let window = Window {
        display: NonNull::new(connection.backend().display_ptr().cast()).unwrap(),
        surface: NonNull::new(surface.id().as_ptr().cast()).unwrap(),
    };
    let video = VideoSurface::attach(&window).expect("attach the video surface");
    video.place(0.0, 0.0, 320.0, 240.0, 1.0);
    surface.commit();
    connection.flush().unwrap();

    let player = EmbeddedPlayer::load().expect("libmpv.so.2");
    let handle = player
        .play(
            PlayRequest::new(common::test_media(), "Surface"),
            video.clone(),
        )
        .expect("play into the surface");
    common::wait_for(&handle, Duration::from_secs(15), |e| {
        *e == PlayerEvent::Started
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    while !video.is_ready() {
        assert!(Instant::now() < deadline, "no frame presented");
        queue.dispatch_pending(&mut app).unwrap();
        std::thread::sleep(Duration::from_millis(20));
    }

    handle.stop();
    match common::wait_for(&handle, Duration::from_secs(10), |e| {
        matches!(e, PlayerEvent::Ended { .. })
    }) {
        PlayerEvent::Ended { reason, .. } => assert_eq!(reason, EndReason::Quit),
        _ => unreachable!(),
    }
    drop(video);
    connection.flush().unwrap();
}
