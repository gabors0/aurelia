//! A minimal binding to `libmpv.so.2`, loaded at runtime so Aurelia starts
//! (and falls back to external mpv) without it. Layouts follow mpv's
//! `client.h`/`render.h` (client API 2.x).

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::Arc;

pub(crate) const EVENT_NONE: c_int = 0;
pub(crate) const EVENT_SHUTDOWN: c_int = 1;
pub(crate) const EVENT_LOG_MESSAGE: c_int = 2;
pub(crate) const EVENT_COMMAND_REPLY: c_int = 5;
pub(crate) const EVENT_START_FILE: c_int = 6;
pub(crate) const EVENT_END_FILE: c_int = 7;
pub(crate) const EVENT_FILE_LOADED: c_int = 8;
pub(crate) const EVENT_IDLE: c_int = 11;
pub(crate) const EVENT_SEEK: c_int = 20;
pub(crate) const EVENT_PLAYBACK_RESTART: c_int = 21;
pub(crate) const EVENT_PROPERTY_CHANGE: c_int = 22;

pub(crate) const FORMAT_NONE: c_int = 0;
pub(crate) const FORMAT_STRING: c_int = 1;
pub(crate) const FORMAT_FLAG: c_int = 3;
pub(crate) const FORMAT_INT64: c_int = 4;
pub(crate) const FORMAT_DOUBLE: c_int = 5;
pub(crate) const FORMAT_NODE: c_int = 6;
pub(crate) const FORMAT_NODE_ARRAY: c_int = 7;
pub(crate) const FORMAT_NODE_MAP: c_int = 8;

pub(crate) const END_FILE_EOF: c_int = 0;
pub(crate) const END_FILE_STOP: c_int = 2;
pub(crate) const END_FILE_QUIT: c_int = 3;
pub(crate) const END_FILE_ERROR: c_int = 4;
pub(crate) const END_FILE_REDIRECT: c_int = 5;

pub(crate) const RENDER_PARAM_INVALID: c_int = 0;
pub(crate) const RENDER_PARAM_API_TYPE: c_int = 1;
pub(crate) const RENDER_PARAM_OPENGL_INIT_PARAMS: c_int = 2;
pub(crate) const RENDER_PARAM_OPENGL_FBO: c_int = 3;
pub(crate) const RENDER_PARAM_FLIP_Y: c_int = 4;
pub(crate) const RENDER_PARAM_WL_DISPLAY: c_int = 9;

#[repr(C)]
pub(crate) struct RawEvent {
    pub event_id: c_int,
    pub error: c_int,
    pub reply_userdata: u64,
    pub data: *mut c_void,
}

#[repr(C)]
pub(crate) struct EventProperty {
    pub name: *const c_char,
    pub format: c_int,
    pub data: *mut c_void,
}

#[repr(C)]
pub(crate) struct EventEndFile {
    pub reason: c_int,
    pub error: c_int,
    pub playlist_entry_id: i64,
    pub playlist_insert_id: i64,
    pub playlist_insert_num_entries: c_int,
}

#[repr(C)]
pub(crate) struct LogMessage {
    pub prefix: *const c_char,
    pub level: *const c_char,
    pub text: *const c_char,
    pub log_level: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) union NodeValue {
    pub string: *mut c_char,
    pub flag: c_int,
    pub int64: i64,
    pub double: f64,
    pub list: *mut NodeList,
    pub byte_array: *mut c_void,
}

#[repr(C)]
pub(crate) struct Node {
    pub u: NodeValue,
    pub format: c_int,
}

#[repr(C)]
pub(crate) struct NodeList {
    pub num: c_int,
    pub values: *mut Node,
    pub keys: *mut *mut c_char,
}

#[repr(C)]
pub(crate) struct RenderParam {
    pub kind: c_int,
    pub data: *mut c_void,
}

pub(crate) type GetProcAddress = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;

#[repr(C)]
pub(crate) struct OpenGlInitParams {
    pub get_proc_address: GetProcAddress,
    pub get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
pub(crate) struct OpenGlFbo {
    pub fbo: c_int,
    pub w: c_int,
    pub h: c_int,
    pub internal_format: c_int,
}

type Handle = *mut c_void;
pub(crate) type UpdateCallback = unsafe extern "C" fn(*mut c_void);

/// The loaded library's entry points.
pub(crate) struct Lib {
    client_api_version: unsafe extern "C" fn() -> u64,
    error_string: unsafe extern "C" fn(c_int) -> *const c_char,
    free: unsafe extern "C" fn(*mut c_void),
    create: unsafe extern "C" fn() -> Handle,
    initialize: unsafe extern "C" fn(Handle) -> c_int,
    terminate_destroy: unsafe extern "C" fn(Handle),
    set_option_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    set_property_string: unsafe extern "C" fn(Handle, *const c_char, *const c_char) -> c_int,
    get_property_string: unsafe extern "C" fn(Handle, *const c_char) -> *mut c_char,
    command_async: unsafe extern "C" fn(Handle, u64, *const *const c_char) -> c_int,
    observe_property: unsafe extern "C" fn(Handle, u64, *const c_char, c_int) -> c_int,
    request_log_messages: unsafe extern "C" fn(Handle, *const c_char) -> c_int,
    wait_event: unsafe extern "C" fn(Handle, f64) -> *mut RawEvent,
    wakeup: unsafe extern "C" fn(Handle),
    render_context_create: unsafe extern "C" fn(*mut Handle, Handle, *mut RenderParam) -> c_int,
    render_context_set_update_callback:
        unsafe extern "C" fn(Handle, Option<UpdateCallback>, *mut c_void),
    render_context_update: unsafe extern "C" fn(Handle) -> u64,
    render_context_render: unsafe extern "C" fn(Handle, *mut RenderParam) -> c_int,
    render_context_report_swap: unsafe extern "C" fn(Handle),
    render_context_free: unsafe extern "C" fn(Handle),
    _library: libloading::Library,
}

impl Lib {
    /// `$AURELIA_LIBMPV`, else `libmpv.so.2` from the library path.
    pub fn load() -> Result<Arc<Lib>, String> {
        let name = std::env::var_os("AURELIA_LIBMPV").unwrap_or_else(|| "libmpv.so.2".into());
        // SAFETY: loading libmpv runs no initialisers with preconditions.
        let library = unsafe { libloading::Library::new(&name) }
            .map_err(|err| format!("{}: {err}", name.to_string_lossy()))?;
        macro_rules! sym {
            ($name:literal) => {
                // SAFETY: the type matches the C declaration in mpv's headers.
                *unsafe { library.get(concat!($name, "\0").as_bytes()) }
                    .map_err(|err| format!("{}: {err}", $name))?
            };
        }
        let lib = Lib {
            client_api_version: sym!("mpv_client_api_version"),
            error_string: sym!("mpv_error_string"),
            free: sym!("mpv_free"),
            create: sym!("mpv_create"),
            initialize: sym!("mpv_initialize"),
            terminate_destroy: sym!("mpv_terminate_destroy"),
            set_option_string: sym!("mpv_set_option_string"),
            set_property_string: sym!("mpv_set_property_string"),
            get_property_string: sym!("mpv_get_property_string"),
            command_async: sym!("mpv_command_async"),
            observe_property: sym!("mpv_observe_property"),
            request_log_messages: sym!("mpv_request_log_messages"),
            wait_event: sym!("mpv_wait_event"),
            wakeup: sym!("mpv_wakeup"),
            render_context_create: sym!("mpv_render_context_create"),
            render_context_set_update_callback: sym!("mpv_render_context_set_update_callback"),
            render_context_update: sym!("mpv_render_context_update"),
            render_context_render: sym!("mpv_render_context_render"),
            render_context_report_swap: sym!("mpv_render_context_report_swap"),
            render_context_free: sym!("mpv_render_context_free"),
            _library: library,
        };
        // SAFETY: no arguments.
        let version = unsafe { (lib.client_api_version)() };
        if version >> 16 != 2 {
            return Err(format!(
                "libmpv client API {}.{} (2.x needed)",
                version >> 16,
                version & 0xffff
            ));
        }
        Ok(Arc::new(lib))
    }

    pub fn error_string(&self, code: c_int) -> String {
        // SAFETY: returns a static string for any code.
        let text = unsafe { (self.error_string)(code) };
        if text.is_null() {
            return format!("error {code}");
        }
        // SAFETY: non-null, NUL-terminated, static.
        unsafe { CStr::from_ptr(text) }
            .to_string_lossy()
            .into_owned()
    }
}

fn c_string(text: &str) -> Result<CString, String> {
    CString::new(text).map_err(|_| format!("{text:?} contains a NUL byte"))
}

/// One mpv core. libmpv is thread-safe, so it's shared between the session's
/// event thread, its control and the render thread; the last owner destroys
/// it (after the render context is gone, which the render thread ensures by
/// holding a reference until it frees its context).
pub(crate) struct Core {
    lib: Arc<Lib>,
    handle: Handle,
}

// SAFETY: every libmpv client API function may be called from any thread.
unsafe impl Send for Core {}
// SAFETY: as above.
unsafe impl Sync for Core {}

impl Core {
    pub fn create(lib: Arc<Lib>) -> Result<Self, String> {
        // SAFETY: no arguments; null on failure.
        let handle = unsafe { (lib.create)() };
        if handle.is_null() {
            return Err("mpv_create failed".into());
        }
        Ok(Self { lib, handle })
    }

    pub fn lib(&self) -> &Lib {
        &self.lib
    }

    fn check(&self, what: &str, code: c_int) -> Result<(), String> {
        if code < 0 {
            Err(format!("{what}: {}", self.lib.error_string(code)))
        } else {
            Ok(())
        }
    }

    /// Before `initialize`: an option. Values are never logged by us.
    pub fn set_option(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (c_string(name)?, c_string(value)?);
        // SAFETY: valid handle and C strings.
        let code = unsafe { (self.lib.set_option_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        self.check(name, code)
    }

    pub fn initialize(&self) -> Result<(), String> {
        // SAFETY: valid handle.
        let code = unsafe { (self.lib.initialize)(self.handle) };
        self.check("mpv_initialize", code)
    }

    pub fn set_property(&self, name: &str, value: &str) -> Result<(), String> {
        let (n, v) = (c_string(name)?, c_string(value)?);
        // SAFETY: valid handle and C strings.
        let code = unsafe { (self.lib.set_property_string)(self.handle, n.as_ptr(), v.as_ptr()) };
        self.check(name, code)
    }

    pub fn get_property(&self, name: &str) -> Option<String> {
        let n = c_string(name).ok()?;
        // SAFETY: valid handle and C string; the result is ours to free.
        let value = unsafe { (self.lib.get_property_string)(self.handle, n.as_ptr()) };
        if value.is_null() {
            return None;
        }
        // SAFETY: non-null, NUL-terminated.
        let text = unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: allocated by libmpv.
        unsafe { (self.lib.free)(value.cast()) };
        Some(text)
    }

    /// Queues a command; failures arrive as command replies. The words are
    /// not logged (they can carry header values).
    pub fn command(&self, words: &[String]) -> Result<(), String> {
        let words = words
            .iter()
            .map(|w| c_string(w))
            .collect::<Result<Vec<_>, _>>()?;
        let mut args: Vec<*const c_char> = words.iter().map(|w| w.as_ptr()).collect();
        args.push(std::ptr::null());
        // SAFETY: a NULL-terminated array of C strings, copied by libmpv.
        let code = unsafe { (self.lib.command_async)(self.handle, 0, args.as_ptr()) };
        self.check("command", code)
    }

    pub fn observe(&self, id: u64, name: &str) -> Result<(), String> {
        let n = c_string(name)?;
        // SAFETY: valid handle and C string.
        let code = unsafe { (self.lib.observe_property)(self.handle, id, n.as_ptr(), FORMAT_NODE) };
        self.check(name, code)
    }

    pub fn request_log(&self, level: &str) -> Result<(), String> {
        let l = c_string(level)?;
        // SAFETY: valid handle and C string.
        let code = unsafe { (self.lib.request_log_messages)(self.handle, l.as_ptr()) };
        self.check("request_log_messages", code)
    }

    /// Waits for the next event. The event stays valid until the next call;
    /// only one thread may wait.
    pub fn wait_event(&self, timeout: f64) -> &RawEvent {
        // SAFETY: never null; valid until the next wait on this handle.
        unsafe { &*(self.lib.wait_event)(self.handle, timeout) }
    }

    pub fn wakeup(&self) {
        // SAFETY: valid handle.
        unsafe { (self.lib.wakeup)(self.handle) }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // SAFETY: the handle is valid and no longer used by anyone.
        unsafe { (self.lib.terminate_destroy)(self.handle) }
    }
}

/// An OpenGL render context for a core. Must be created, used and dropped
/// on one thread with the GL context current; it keeps the core alive.
pub(crate) struct RenderContext {
    core: Arc<Core>,
    handle: Handle,
}

impl RenderContext {
    /// # Safety
    /// A GL context must be current on this thread; `wl_display` must be the
    /// display that context's surfaces live on.
    pub unsafe fn create(
        core: Arc<Core>,
        get_proc_address: GetProcAddress,
        wl_display: *mut c_void,
    ) -> Result<Self, String> {
        let mut init = OpenGlInitParams {
            get_proc_address,
            get_proc_address_ctx: std::ptr::null_mut(),
        };
        let api = c"opengl";
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_API_TYPE,
                data: api.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: RENDER_PARAM_OPENGL_INIT_PARAMS,
                data: (&raw mut init).cast(),
            },
            // Lets hardware decoding share frames with EGL (VA-API dmabufs).
            RenderParam {
                kind: RENDER_PARAM_WL_DISPLAY,
                data: wl_display,
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        let mut handle: Handle = std::ptr::null_mut();
        // SAFETY: params are valid for the call; GL is current (caller).
        let code = unsafe {
            (core.lib.render_context_create)(&mut handle, core.handle, params.as_mut_ptr())
        };
        core.check("mpv_render_context_create", code)?;
        Ok(Self { core, handle })
    }

    /// # Safety
    /// `data` must stay valid until this context is dropped.
    pub unsafe fn set_update_callback(&self, callback: UpdateCallback, data: *mut c_void) {
        // SAFETY: valid context; the caller keeps `data` alive.
        unsafe {
            (self.core.lib.render_context_set_update_callback)(self.handle, Some(callback), data)
        }
    }

    pub fn update(&self) -> u64 {
        // SAFETY: valid context, on its thread.
        unsafe { (self.core.lib.render_context_update)(self.handle) }
    }

    /// Draws the current frame into the default framebuffer.
    pub fn render(&self, width: i32, height: i32) -> Result<(), String> {
        let mut fbo = OpenGlFbo {
            fbo: 0,
            w: width,
            h: height,
            internal_format: 0,
        };
        let mut flip: c_int = 1;
        let mut params = [
            RenderParam {
                kind: RENDER_PARAM_OPENGL_FBO,
                data: (&raw mut fbo).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_FLIP_Y,
                data: (&raw mut flip).cast(),
            },
            RenderParam {
                kind: RENDER_PARAM_INVALID,
                data: std::ptr::null_mut(),
            },
        ];
        // SAFETY: valid context and params, GL current (create's contract).
        let code =
            unsafe { (self.core.lib.render_context_render)(self.handle, params.as_mut_ptr()) };
        self.core.check("render", code)
    }

    pub fn report_swap(&self) {
        // SAFETY: valid context.
        unsafe { (self.core.lib.render_context_report_swap)(self.handle) }
    }
}

impl Drop for RenderContext {
    fn drop(&mut self) {
        // SAFETY: valid context, freed once, on its thread with GL current.
        unsafe { (self.core.lib.render_context_free)(self.handle) }
    }
}
