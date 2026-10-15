//! Where embedded video is drawn: a `wl_subsurface` placed *below* the app
//! window's own surface. The app keeps the area over it transparent and draws
//! its controls there; the compositor puts the two together.
//!
//! The subsurface lives on the app's Wayland connection (a foreign display)
//! with its own event queue. A render thread owns EGL: it clears to black
//! until a core is attached, then draws mpv's frames whenever mpv asks.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, mpsc};
use std::thread::JoinHandle;

use khronos_egl as egl;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{
    wl_compositor::WlCompositor, wl_region::WlRegion, wl_registry::WlRegistry,
    wl_subcompositor::WlSubcompositor, wl_subsurface::WlSubsurface, wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};

use super::EmbedError;
use super::libmpv::{Core, RenderContext};

const EGL_PLATFORM_WAYLAND_KHR: egl::Enum = 0x31D8;

type Egl = egl::DynamicInstance<egl::EGL1_5>;

/// libEGL, loaded once; mpv's `get_proc_address` needs it without a context.
fn egl_instance() -> Result<&'static Egl, String> {
    static EGL: OnceLock<Result<Egl, String>> = OnceLock::new();
    EGL.get_or_init(|| {
        // SAFETY: loading libEGL runs no initialisers with preconditions.
        unsafe { Egl::load_required() }.map_err(|err| format!("libEGL: {err}"))
    })
    .as_ref()
    .map_err(Clone::clone)
}

unsafe extern "C" fn get_proc_address(_: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: mpv passes a NUL-terminated name.
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        return std::ptr::null_mut();
    };
    egl_instance()
        .ok()
        .and_then(|egl| egl.get_proc_address(name))
        .map_or(std::ptr::null_mut(), |f| f as *mut c_void)
}

/// A pointer handed to the render thread.
#[derive(Clone, Copy)]
struct SendPtr(*mut c_void);
// SAFETY: a `wl_display*`, which libwayland lets every thread use.
unsafe impl Send for SendPtr {}
// SAFETY: as above.
unsafe impl Sync for SendPtr {}

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
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
delegate_noop!(State: WlCompositor);
delegate_noop!(State: WlSubcompositor);
delegate_noop!(State: WlSubsurface);
delegate_noop!(State: WlRegion);
delegate_noop!(State: WpViewporter);
delegate_noop!(State: WpViewport);
// enter/leave/preferred scale: the app's layout already tells us the scale.
delegate_noop!(State: ignore WlSurface);

/// What the render thread is asked to do.
enum Request {
    Attach(Arc<Core>),
    Detach,
}

#[derive(Default)]
struct RenderState {
    /// Physical size; zero until the surface is first placed.
    size: (i32, i32),
    dirty: bool,
    request: Option<Request>,
    reply: Option<Result<(), String>>,
    quit: bool,
    /// The render thread has exited.
    gone: bool,
}

struct Shared {
    state: Mutex<RenderState>,
    wake: Condvar,
    /// A frame has been presented.
    ready: AtomicBool,
}

/// Logical position and size, and physical size.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Placement {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    physical: (i32, i32),
}

/// A video surface below a Wayland window. Dropping it removes it.
pub struct VideoSurface {
    connection: Connection,
    queue: Mutex<EventQueue<State>>,
    queue_handle: QueueHandle<State>,
    compositor: WlCompositor,
    surface: WlSurface,
    subsurface: WlSubsurface,
    viewport: WpViewport,
    placed: Mutex<Option<Placement>>,
    shared: Arc<Shared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl VideoSurface {
    /// Puts a video surface under `window`'s surface and starts its render
    /// thread. Fails on anything but Wayland, or if EGL can't be set up.
    pub fn attach(
        window: &(impl HasWindowHandle + HasDisplayHandle),
    ) -> Result<Arc<Self>, EmbedError> {
        let surface_error =
            |what: &str, err: &dyn std::fmt::Display| EmbedError::Surface(format!("{what}: {err}"));
        let display = match window.display_handle().map(|h| h.as_raw()) {
            Ok(RawDisplayHandle::Wayland(handle)) => handle.display.as_ptr(),
            Ok(_) => return Err(EmbedError::NotWayland),
            Err(err) => return Err(surface_error("display handle", &err)),
        };
        let parent = match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Wayland(handle)) => handle.surface.as_ptr(),
            Ok(_) => return Err(EmbedError::NotWayland),
            Err(err) => return Err(surface_error("window handle", &err)),
        };

        // SAFETY: the window's live `wl_display*`; it outlives this surface
        // because the window does.
        let backend = unsafe { Backend::from_foreign_display(display.cast()) };
        let connection = Connection::from_backend(backend);
        let (globals, queue) = registry_queue_init::<State>(&connection)
            .map_err(|err| surface_error("registry", &err))?;
        let qh = queue.handle();
        let compositor: WlCompositor = globals
            .bind(&qh, 4..=6, ())
            .map_err(|err| surface_error("wl_compositor", &err))?;
        let subcompositor: WlSubcompositor = globals
            .bind(&qh, 1..=1, ())
            .map_err(|err| surface_error("wl_subcompositor", &err))?;
        let viewporter: WpViewporter = globals
            .bind(&qh, 1..=1, ())
            .map_err(|err| surface_error("wp_viewporter", &err))?;

        // SAFETY: the window's live `wl_surface*` on this display.
        let parent_id = unsafe { ObjectId::from_ptr(WlSurface::interface(), parent.cast()) }
            .map_err(|err| surface_error("window surface", &err))?;
        let parent = WlSurface::from_id(&connection, parent_id)
            .map_err(|err| surface_error("window surface", &err))?;

        let surface = compositor.create_surface(&qh, ());
        let subsurface = subcompositor.get_subsurface(&surface, &parent, &qh, ());
        // Takes effect with the window's next commit, i.e. its next frame.
        subsurface.place_below(&parent);
        // Frames go up when mpv renders them, not with the window's commits.
        subsurface.set_desync();
        // Pointer input passes through to the window.
        let region = compositor.create_region(&qh, ());
        surface.set_input_region(Some(&region));
        region.destroy();
        let viewport = viewporter.get_viewport(&surface, &qh, ());
        subcompositor.destroy();
        viewporter.destroy();
        connection
            .flush()
            .map_err(|err| surface_error("flush", &err))?;

        let shared = Arc::new(Shared {
            state: Mutex::new(RenderState::default()),
            wake: Condvar::new(),
            ready: AtomicBool::new(false),
        });
        let (init_tx, init_rx) = mpsc::channel();
        let thread = {
            let shared = shared.clone();
            let display = SendPtr(display);
            let id = surface.id();
            std::thread::Builder::new()
                .name("video-render".into())
                .spawn(move || render_thread(shared, display, id, init_tx))
                .map_err(|err| surface_error("render thread", &err))?
        };
        let this = Arc::new(Self {
            connection,
            queue: Mutex::new(queue),
            queue_handle: qh,
            compositor,
            surface,
            subsurface,
            viewport,
            placed: Mutex::new(None),
            shared,
            thread: Mutex::new(Some(thread)),
        });
        match init_rx.recv() {
            Ok(Ok(())) => Ok(this),
            Ok(Err(err)) => Err(EmbedError::Surface(err)),
            Err(_) => Err(EmbedError::Surface("the render thread stopped".into())),
        }
    }

    /// Moves and sizes the video to `x, y, width × height` in the window's
    /// logical pixels at `scale`. Call from the window's thread as it lays
    /// out; it takes effect with the window's next frame.
    pub fn place(&self, x: f32, y: f32, width: f32, height: f32, scale: f32) {
        // Events for our objects (surface enter/leave) collect on our queue;
        // the app's event loop reads the socket but only we dispatch them.
        if let Err(err) = self.queue.lock().unwrap().dispatch_pending(&mut State) {
            tracing::debug!("video surface events: {err}");
        }
        let (width, height) = (width.round() as i32, height.round() as i32);
        if width <= 0 || height <= 0 {
            return;
        }
        let placement = Placement {
            x: x.round() as i32,
            y: y.round() as i32,
            width,
            height,
            physical: (
                (width as f32 * scale).round() as i32,
                (height as f32 * scale).round() as i32,
            ),
        };
        {
            let mut placed = self.placed.lock().unwrap();
            if *placed == Some(placement) {
                return;
            }
            *placed = Some(placement);
        }
        tracing::debug!(
            "video at {},{} {width}×{height} ({}×{} pixels)",
            placement.x,
            placement.y,
            placement.physical.0,
            placement.physical.1
        );
        self.subsurface.set_position(placement.x, placement.y);
        // The buffer is physical-sized; the viewport maps it onto logical
        // pixels, so fractional scales stay sharp.
        self.viewport.set_destination(width, height);
        let region = self.compositor.create_region(&self.queue_handle, ());
        region.add(0, 0, width, height);
        self.surface.set_opaque_region(Some(&region));
        region.destroy();
        let _ = self.connection.flush();

        let mut state = self.shared.state.lock().unwrap();
        state.size = placement.physical;
        state.dirty = true;
        self.shared.wake.notify_all();
    }

    /// Whether a frame (black, or video) has been presented: from then on the
    /// window may show the area above it as transparent.
    pub fn is_ready(&self) -> bool {
        self.shared.ready.load(Ordering::Acquire)
    }

    /// Starts drawing `core`'s video: creates its render context on the
    /// render thread. Blocks until that's done.
    pub(crate) fn attach_core(&self, core: Arc<Core>) -> Result<(), String> {
        self.request(Request::Attach(core))
    }

    /// Stops drawing video and frees the render context (on the render
    /// thread). Blocks until that's done; must come before the core is
    /// destroyed.
    pub(crate) fn detach_core(&self) {
        if let Err(err) = self.request(Request::Detach) {
            tracing::debug!("detaching video: {err}");
        }
    }

    fn request(&self, request: Request) -> Result<(), String> {
        let mut state = self.shared.state.lock().unwrap();
        if state.gone {
            return Err("the render thread stopped".into());
        }
        state.reply = None;
        state.request = Some(request);
        self.shared.wake.notify_all();
        loop {
            if let Some(reply) = state.reply.take() {
                return reply;
            }
            if state.gone {
                return Err("the render thread stopped".into());
            }
            state = self.shared.wake.wait(state).unwrap();
        }
    }
}

impl Drop for VideoSurface {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.quit = true;
            self.shared.wake.notify_all();
        }
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
        self.viewport.destroy();
        self.subsurface.destroy();
        self.surface.destroy();
        let _ = self.connection.flush();
    }
}

/// mpv's "a new frame is due" callback; may come from any thread.
unsafe extern "C" fn on_update(data: *mut c_void) {
    // SAFETY: `Shared`, kept alive by the render thread until the render
    // context (which calls this) is freed.
    let shared = unsafe { &*(data as *const Shared) };
    shared.state.lock().unwrap().dirty = true;
    shared.wake.notify_all();
}

fn render_thread(
    shared: Arc<Shared>,
    display: SendPtr,
    surface: ObjectId,
    init: mpsc::Sender<Result<(), String>>,
) {
    let gl = match Gl::new(display.0, surface) {
        Ok(gl) => {
            let _ = init.send(Ok(()));
            gl
        }
        Err(err) => {
            let _ = init.send(Err(err));
            shared.state.lock().unwrap().gone = true;
            shared.wake.notify_all();
            return;
        }
    };
    let mut context: Option<RenderContext> = None;
    let mut current = (1, 1);
    loop {
        // Never hold the lock while calling into mpv: it may call
        // `on_update` from inside.
        let (request, size) = {
            let mut state = shared.state.lock().unwrap();
            while !state.dirty && state.request.is_none() && !state.quit {
                state = shared.wake.wait(state).unwrap();
            }
            if state.quit {
                break;
            }
            state.dirty = false;
            (state.request.take(), state.size)
        };

        if let Some(request) = request {
            let reply = match request {
                Request::Attach(core) => {
                    context = None;
                    // SAFETY: our GL context is current on this thread, on
                    // surfaces of this display.
                    match unsafe { RenderContext::create(core, get_proc_address, display.0) } {
                        Ok(created) => {
                            // SAFETY: `shared` outlives the context (freed
                            // below, before this thread lets go of it).
                            unsafe {
                                created.set_update_callback(
                                    on_update,
                                    Arc::as_ptr(&shared) as *mut c_void,
                                )
                            };
                            context = Some(created);
                            Ok(())
                        }
                        Err(err) => Err(err),
                    }
                }
                Request::Detach => {
                    context = None;
                    Ok(())
                }
            };
            let mut state = shared.state.lock().unwrap();
            state.reply = Some(reply);
            // Draw once more: black after a detach, the first frame after
            // an attach.
            state.dirty = true;
            shared.wake.notify_all();
            continue;
        }

        if size.0 <= 0 || size.1 <= 0 {
            continue;
        }
        if size != current {
            gl.window.resize(size.0, size.1, 0, 0);
            current = size;
        }
        match &context {
            Some(context) => {
                context.update();
                if let Err(err) = context.render(current.0, current.1) {
                    tracing::warn!("video frame: {err}");
                }
            }
            None => gl.clear_black(),
        }
        // `swap_interval(0)`: a hidden window gets no frame callbacks, so a
        // blocking swap could stall forever.
        if let Err(err) = gl.swap() {
            tracing::warn!("video swap: {err}");
            continue;
        }
        if let Some(context) = &context {
            context.report_swap();
        }
        if !shared.ready.swap(true, Ordering::Release) {
            tracing::debug!("first video frame presented");
        }
    }
    // Free mpv's GL objects while the context is still current.
    drop(context);
    drop(gl);
    shared.state.lock().unwrap().gone = true;
    shared.wake.notify_all();
}

type GlClearColor = unsafe extern "C" fn(f32, f32, f32, f32);
type GlClear = unsafe extern "C" fn(u32);
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;

/// The render thread's EGL context and window surface.
struct Gl {
    egl: &'static Egl,
    display: egl::Display,
    context: egl::Context,
    surface: egl::Surface,
    window: wayland_egl::WlEglSurface,
    clear_color: GlClearColor,
    clear: GlClear,
}

impl Gl {
    fn new(wl_display: *mut c_void, surface: ObjectId) -> Result<Self, String> {
        let egl = egl_instance()?;
        // SAFETY: a live `wl_display*`. EGL hands back the same EGLDisplay to
        // anyone else on this display, so it's never terminated here.
        let display = unsafe {
            egl.get_platform_display(EGL_PLATFORM_WAYLAND_KHR, wl_display, &[egl::ATTRIB_NONE])
        }
        .map_err(|err| format!("EGL display: {err}"))?;
        egl.initialize(display)
            .map_err(|err| format!("EGL initialize: {err}"))?;
        egl.bind_api(egl::OPENGL_API)
            .map_err(|err| format!("EGL OpenGL API: {err}"))?;

        let attributes = [
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::NONE,
        ];
        let mut configs = Vec::with_capacity(64);
        egl.choose_config(display, &attributes, &mut configs)
            .map_err(|err| format!("EGL config: {err}"))?;
        // Opaque (XRGB) if there is one: the desktop must never show through
        // the video.
        let config = configs
            .iter()
            .copied()
            .find(|c| matches!(egl.get_config_attrib(display, *c, egl::ALPHA_SIZE), Ok(0)))
            .or_else(|| configs.first().copied())
            .ok_or("no EGL config for OpenGL windows")?;

        let core_profile = [
            egl::CONTEXT_MAJOR_VERSION,
            3,
            egl::CONTEXT_MINOR_VERSION,
            3,
            egl::CONTEXT_OPENGL_PROFILE_MASK,
            egl::CONTEXT_OPENGL_CORE_PROFILE_BIT,
            egl::NONE,
        ];
        let context = egl
            .create_context(display, config, None, &core_profile)
            .or_else(|_| egl.create_context(display, config, None, &[egl::NONE]))
            .map_err(|err| format!("EGL context: {err}"))?;

        let window = wayland_egl::WlEglSurface::new(surface, 1, 1)
            .map_err(|err| format!("wl_egl_window: {err}"))?;
        // SAFETY: a live `wl_egl_window*`, kept alive by `Gl`.
        let egl_surface = unsafe {
            egl.create_window_surface(display, config, window.ptr() as egl::NativeWindowType, None)
        }
        .map_err(|err| format!("EGL surface: {err}"))?;
        egl.make_current(display, Some(egl_surface), Some(egl_surface), Some(context))
            .map_err(|err| format!("EGL make current: {err}"))?;
        egl.swap_interval(display, 0)
            .map_err(|err| format!("EGL swap interval: {err}"))?;

        let load = |name: &str| egl.get_proc_address(name).ok_or(format!("GL lacks {name}"));
        // SAFETY: the GL signatures of these entry points.
        let clear_color: GlClearColor = unsafe { std::mem::transmute(load("glClearColor")?) };
        // SAFETY: as above.
        let clear: GlClear = unsafe { std::mem::transmute(load("glClear")?) };
        Ok(Self {
            egl,
            display,
            context,
            surface: egl_surface,
            window,
            clear_color,
            clear,
        })
    }

    fn clear_black(&self) {
        // SAFETY: our context is current on this thread.
        unsafe {
            (self.clear_color)(0.0, 0.0, 0.0, 1.0);
            (self.clear)(GL_COLOR_BUFFER_BIT);
        }
    }

    fn swap(&self) -> Result<(), egl::Error> {
        self.egl.swap_buffers(self.display, self.surface)
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        let _ = self.egl.make_current(self.display, None, None, None);
        let _ = self.egl.destroy_surface(self.display, self.surface);
        let _ = self.egl.destroy_context(self.display, self.context);
        // `window` (the wl_egl_window) drops after this, as it must.
    }
}
