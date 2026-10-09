mod accounts;
mod components;
mod dev;
mod format;
mod images;
mod loadable;
mod nav;
mod playback;
mod runtime;
mod session;
mod settings;
mod state;
mod theme;
mod user_data;
mod views;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("AURELIA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,aurelia=info,player=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            runtime::init(cx);
            state::AppState::init(cx);
            settings::init(state::AppState::global(cx).store().dir().to_path_buf(), cx);
            components::key_nav::init(cx);
            images::ImageStore::init(cx);
            views::shell::bind_keys(cx);
            views::profiles::bind_keys(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
                window_min_size: Some(size(px(960.), px(600.))),
                window_decorations: Some(WindowDecorations::Client),
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("dev.aurelia.Aurelia".into()),
                titlebar: Some(TitlebarOptions {
                    title: Some("Aurelia".into()),
                    ..TitleBar::title_bar_options()
                }),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| views::app::AppRoot::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
