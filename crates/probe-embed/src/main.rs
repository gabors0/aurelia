//! THROWAWAY feasibility probe: libmpv rendering into a Wayland subsurface
//! placed below a transparent GPUI window, with GPUI content drawn on top.
//!
//! Usage: probe-embed <url>

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use gpui_kit::*;
use khronos_egl as egl;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
    wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter};

// ---------------------------------------------------------------- libmpv ---

#[repr(C)]
struct MpvEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}
#[repr(C)]
struct LogMessage {
    prefix: *const c_char,
    level: *const c_char,
    text: *const c_char,
}
#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}
#[repr(C)]
struct GlInit {
    get_proc_address: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    ctx: *mut c_void,
}
#[repr(C)]
struct Fbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

struct Api {
    _lib: libloading::Library,
    create: unsafe extern "C" fn() -> *mut c_void,
    initialize: unsafe extern "C" fn(*mut c_void) -> c_int,
    set_option_string: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int,
    command: unsafe extern "C" fn(*mut c_void, *const *const c_char) -> c_int,
    wait_event: unsafe extern "C" fn(*mut c_void, f64) -> *mut MpvEvent,
    request_log: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    render_create: unsafe extern "C" fn(*mut *mut c_void, *mut c_void, *mut RenderParam) -> c_int,
    render_set_update_cb:
        unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
    render_update: unsafe extern "C" fn(*mut c_void) -> u64,
    render_render: unsafe extern "C" fn(*mut c_void, *mut RenderParam) -> c_int,
    render_report_swap: unsafe extern "C" fn(*mut c_void),
}

impl Api {
    fn load() -> Self {
        unsafe {
            let lib = libloading::Library::new("libmpv.so.2").expect("libmpv.so.2");
            macro_rules! sym {
                ($name:literal) => {
                    *lib.get(concat!($name, "\0").as_bytes()).expect($name)
                };
            }
            Self {
                create: sym!("mpv_create"),
                initialize: sym!("mpv_initialize"),
                set_option_string: sym!("mpv_set_option_string"),
                command: sym!("mpv_command"),
                wait_event: sym!("mpv_wait_event"),
                request_log: sym!("mpv_request_log_messages"),
                error_string: sym!("mpv_error_string"),
                render_create: sym!("mpv_render_context_create"),
                render_set_update_cb: sym!("mpv_render_context_set_update_callback"),
                render_update: sym!("mpv_render_context_update"),
                render_render: sym!("mpv_render_context_render"),
                render_report_swap: sym!("mpv_render_context_report_swap"),
                _lib: lib,
            }
        }
    }
}

static API: OnceLock<Api> = OnceLock::new();
static EGL: OnceLock<egl::DynamicInstance<egl::EGL1_5>> = OnceLock::new();

fn api() -> &'static Api {
    API.get_or_init(Api::load)
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let name = unsafe { CStr::from_ptr(name) }.to_str().unwrap_or("");
    EGL.get()
        .and_then(|e| e.get_proc_address(name))
        .map(|f| f as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

#[derive(Clone, Copy)]
struct SendPtr(*mut c_void);
unsafe impl Send for SendPtr {}
unsafe impl Sync for SendPtr {}

fn check(what: &str, code: c_int) {
    if code < 0 {
        let msg = unsafe { CStr::from_ptr((api().error_string)(code)) };
        eprintln!("[probe] {what} failed: {}", msg.to_string_lossy());
    }
}

// --------------------------------------------------------------- wayland ---

struct WlState;
impl Dispatch<WlRegistry, GlobalListContents> for WlState {
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
delegate_noop!(WlState: WlCompositor);
delegate_noop!(WlState: WlSubcompositor);
delegate_noop!(WlState: WlSubsurface);
delegate_noop!(WlState: WlRegion);
delegate_noop!(WlState: WpViewporter);
delegate_noop!(WlState: WpViewport);
delegate_noop!(WlState: ignore WlSurface);

#[derive(Default)]
struct Shared {
    /// Physical size requested by the UI thread.
    size: (i32, i32),
    dirty: bool,
}

struct Embed {
    conn: Connection,
    subsurface: WlSubsurface,
    viewport: WpViewport,
    shared: Arc<(Mutex<Shared>, Condvar)>,
    last: Option<(i32, i32, i32, i32, i32, i32)>,
    _queue: wayland_client::EventQueue<WlState>,
}

impl Embed {
    fn start(window: &Window, url: String) -> Self {
        let RawDisplayHandle::Wayland(d) = window.display_handle().unwrap().as_raw() else {
            panic!("not wayland")
        };
        let RawWindowHandle::Wayland(w) = HasWindowHandle::window_handle(window).unwrap().as_raw() else {
            panic!("not wayland")
        };
        let display_ptr = d.display.as_ptr();
        let surface_ptr = w.surface.as_ptr();

        let conn = Connection::from_backend(unsafe { Backend::from_foreign_display(display_ptr.cast()) });
        let (globals, queue) = registry_queue_init::<WlState>(&conn).expect("registry");
        let qh = queue.handle();
        let compositor: WlCompositor = globals.bind(&qh, 4..=6, ()).expect("wl_compositor");
        let subcompositor: WlSubcompositor = globals.bind(&qh, 1..=1, ()).expect("wl_subcompositor");
        let viewporter: WpViewporter = globals.bind(&qh, 1..=1, ()).expect("wp_viewporter");

        let parent_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface_ptr.cast()) }.unwrap();
        let parent = WlSurface::from_id(&conn, parent_id).unwrap();

        let child = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&child, &parent, &qh, ());
        subsurface.place_below(&parent);
        subsurface.set_desync();
        let region = compositor.create_region(&qh, ());
        child.set_input_region(Some(&region));
        region.destroy();
        let viewport = viewporter.get_viewport(&child, &qh, ());
        conn.flush().ok();

        let shared = Arc::new((Mutex::new(Shared { size: (64, 64), dirty: true }), Condvar::new()));
        let child_id = child.id();
        let display = SendPtr(display_ptr);
        let thread_shared = shared.clone();
        std::thread::spawn(move || render_thread(display, child_id, thread_shared, url));

        Self { conn, subsurface, viewport, shared, last: None, _queue: queue }
    }

    fn place(&mut self, bounds: Bounds<Pixels>, scale: f32) {
        let x = f32::from(bounds.origin.x).round() as i32;
        let y = f32::from(bounds.origin.y).round() as i32;
        let w = f32::from(bounds.size.width).round() as i32;
        let h = f32::from(bounds.size.height).round() as i32;
        let pw = (w as f32 * scale).round() as i32;
        let ph = (h as f32 * scale).round() as i32;
        let key = (x, y, w, h, pw, ph);
        if self.last == Some(key) || w <= 0 || h <= 0 {
            return;
        }
        self.last = Some(key);
        eprintln!("[probe] place at {x},{y} {w}x{h} logical, {pw}x{ph} physical");
        self.subsurface.set_position(x, y);
        self.viewport.set_destination(w, h);
        self.conn.flush().ok();
        let (lock, cvar) = &*self.shared;
        let mut s = lock.lock().unwrap();
        s.size = (pw, ph);
        s.dirty = true;
        cvar.notify_one();
    }
}

unsafe extern "C" fn on_update(ctx: *mut c_void) {
    let shared = unsafe { &*(ctx as *const (Mutex<Shared>, Condvar)) };
    shared.0.lock().unwrap().dirty = true;
    shared.1.notify_one();
}

fn render_thread(display: SendPtr, surface: ObjectId, shared: Arc<(Mutex<Shared>, Condvar)>, url: String) {
    let egl = EGL.get_or_init(|| unsafe { egl::DynamicInstance::<egl::EGL1_5>::load_required() }.expect("libEGL"));
    let edisplay = unsafe { egl.get_platform_display(0x31D8, display.0, &[egl::ATTRIB_NONE]) }.expect("egl display");
    let version = egl.initialize(edisplay).expect("egl init");
    eprintln!("[probe] EGL {version:?}");
    egl.bind_api(egl::OPENGL_API).expect("bind GL");
    let config = egl
        .choose_first_config(
            edisplay,
            &[
                egl::RED_SIZE, 8, egl::GREEN_SIZE, 8, egl::BLUE_SIZE, 8,
                egl::SURFACE_TYPE, egl::WINDOW_BIT,
                egl::RENDERABLE_TYPE, egl::OPENGL_BIT,
                egl::NONE,
            ],
        )
        .expect("config")
        .expect("no config");
    let context = egl
        .create_context(
            edisplay,
            config,
            None,
            &[
                egl::CONTEXT_MAJOR_VERSION, 3, egl::CONTEXT_MINOR_VERSION, 3,
                egl::CONTEXT_OPENGL_PROFILE_MASK, egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
                egl::NONE,
            ],
        )
        .expect("context");
    let (mut w, mut h) = shared.0.lock().unwrap().size;
    let egl_window = wayland_egl::WlEglSurface::new(surface, w, h).expect("wl_egl_window");
    let esurface = unsafe {
        egl.create_window_surface(edisplay, config, egl_window.ptr() as egl::NativeWindowType, None)
    }
    .expect("egl surface");
    egl.make_current(edisplay, Some(esurface), Some(esurface), Some(context)).expect("make current");
    egl.swap_interval(edisplay, 0).ok();

    // mpv core
    let a = api();
    let mpv = unsafe { (a.create)() };
    let opt = |k: &str, v: &str| {
        let (k, v) = (CString::new(k).unwrap(), CString::new(v).unwrap());
        check(k.to_str().unwrap(), unsafe { (a.set_option_string)(mpv, k.as_ptr(), v.as_ptr()) });
    };
    opt("vo", "libmpv");
    opt("hwdec", "auto-safe");
    opt("config", "no");
    opt("terminal", "no");
    opt("keep-open", "yes");
    check("initialize", unsafe { (a.initialize)(mpv) });
    check("log", unsafe { (a.request_log)(mpv, c"v".as_ptr()) });

    let mut init = GlInit { get_proc_address, ctx: std::ptr::null_mut() };
    let api_type = c"opengl";
    let mut params = [
        RenderParam { kind: 1, data: api_type.as_ptr() as *mut c_void },
        RenderParam { kind: 2, data: &mut init as *mut _ as *mut c_void },
        RenderParam { kind: 9, data: display.0 },
        RenderParam { kind: 0, data: std::ptr::null_mut() },
    ];
    let mut render: *mut c_void = std::ptr::null_mut();
    check("render ctx", unsafe { (a.render_create)(&mut render, mpv, params.as_mut_ptr()) });
    unsafe {
        (a.render_set_update_cb)(render, Some(on_update), Arc::as_ptr(&shared) as *mut c_void)
    };

    let mpv_ptr = SendPtr(mpv);
    std::thread::spawn(move || event_thread(mpv_ptr));

    let url = CString::new(url).unwrap();
    let args = [c"loadfile".as_ptr(), url.as_ptr(), std::ptr::null()];
    check("loadfile", unsafe { (a.command)(mpv, args.as_ptr()) });

    let mut frames = 0u64;
    loop {
        let size = {
            let mut s = shared.0.lock().unwrap();
            while !s.dirty {
                s = shared.1.wait(s).unwrap();
            }
            s.dirty = false;
            s.size
        };
        if size != (w, h) {
            (w, h) = size;
            egl_window.resize(w, h, 0, 0);
        }
        let flags = unsafe { (a.render_update)(render) };
        let _ = flags;
        let mut fbo = Fbo { fbo: 0, w, h, internal_format: 0 };
        let mut flip: c_int = 1;
        let mut rp = [
            RenderParam { kind: 3, data: &mut fbo as *mut _ as *mut c_void },
            RenderParam { kind: 4, data: &mut flip as *mut _ as *mut c_void },
            RenderParam { kind: 0, data: std::ptr::null_mut() },
        ];
        check("render", unsafe { (a.render_render)(render, rp.as_mut_ptr()) });
        egl.swap_buffers(edisplay, esurface).expect("swap");
        unsafe { (a.render_report_swap)(render) };
        frames += 1;
        if frames % 120 == 1 {
            eprintln!("[probe] frames rendered: {frames} ({w}x{h})");
        }
    }
}

fn event_thread(mpv: SendPtr) {
    let a = api();
    loop {
        let ev = unsafe { &*(a.wait_event)(mpv.0, -1.0) };
        match ev.event_id {
            0 => {}
            1 => return,
            2 => {
                let m = unsafe { &*(ev.data as *const LogMessage) };
                let (p, l, t) = unsafe {
                    (CStr::from_ptr(m.prefix), CStr::from_ptr(m.level), CStr::from_ptr(m.text))
                };
                let t = t.to_string_lossy();
                if l.to_bytes() != b"v" || t.contains("hwdec") || t.contains("Using") {
                    eprint!("[mpv {}/{}] {}", p.to_string_lossy(), l.to_string_lossy(), t);
                }
            }
            id => eprintln!("[probe] mpv event {id} (error {})", ev.error),
        }
    }
}

// ------------------------------------------------------------------- UI ----

struct Probe {
    url: String,
    embed: Option<Arc<Mutex<Embed>>>,
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if std::env::var_os("PROBE_NO_EMBED").is_some() {
            return div().size_full().child("no embed").into_any_element();
        }
        if self.embed.is_none() {
            self.embed = Some(Arc::new(Mutex::new(Embed::start(window, self.url.clone()))));
        }
        let embed = self.embed.clone().unwrap();
        div()
            .size_full()
            .relative()
            .child(
                canvas(
                    move |bounds, window, _| {
                        embed.lock().unwrap().place(bounds, window.scale_factor());
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top(px(60.))
                .left(px(40.))
                .right(px(40.))
                .bottom(px(40.)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(360.))
                    .h(px(50.))
                    .bg(rgb(0xb03030))
                    .text_color(rgb(0xffffff))
                    .child("opaque GPUI box (top-left)"),
            )
            .child(
                div()
                    .absolute()
                    .bottom(px(80.))
                    .left(px(120.))
                    .right(px(120.))
                    .h(px(90.))
                    .rounded(px(18.))
                    .bg(rgba(0x10101099))
                    .text_color(rgb(0xffffff))
                    .text_size(px(28.))
                    .p_4()
                    .child("GPUI controls over video (60% black)"),
            )
            .into_any_element()
    }
}

fn main() {
    let url = std::env::args().nth(1).filter(|s| !s.is_empty()).unwrap_or_else(|| "av://lavfi:testsrc2=size=1920x1080:rate=30".into());
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::update(cx, |theme| {
            theme.colors.background = gpui_kit::Hsla::transparent_black();
        });
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
            window_decorations: Some(WindowDecorations::Client),
            window_background: WindowBackgroundAppearance::Transparent,
            app_id: Some("dev.aurelia.Probe".into()),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Probe { url, embed: None })).unwrap();
        cx.activate(true);
    });
}
