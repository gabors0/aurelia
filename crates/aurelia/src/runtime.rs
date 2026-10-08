//! A small Tokio runtime for network I/O (reqwest needs Tokio); GPUI tasks
//! await its results.

use std::future::Future;

use gpui_kit::{App, Global};

struct NetRuntime(tokio::runtime::Runtime);

impl Global for NetRuntime {}

pub fn init(cx: &mut App) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("aurelia-net")
        .enable_all()
        .build()
        .expect("failed to start the network runtime");
    cx.set_global(NetRuntime(runtime));
}

/// Runs an API call on the network runtime.
pub fn api<T, F>(cx: &App, future: F) -> impl Future<Output = jellyfin::Result<T>> + use<T, F>
where
    T: Send + 'static,
    F: Future<Output = jellyfin::Result<T>> + Send + 'static,
{
    let handle = cx.global::<NetRuntime>().0.spawn(future);
    async move {
        handle
            .await
            .unwrap_or_else(|err| Err(jellyfin::Error::Network(format!("task failed: {err}"))))
    }
}

/// Runs any future on the network runtime; `None` if it panicked.
pub fn run<T, F>(cx: &App, future: F) -> impl Future<Output = Option<T>> + use<T, F>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    let handle = cx.global::<NetRuntime>().0.spawn(future);
    async move { handle.await.ok() }
}
