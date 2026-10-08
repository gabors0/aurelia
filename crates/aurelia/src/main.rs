mod components;
mod format;
mod images;
mod loadable;
mod nav;
mod runtime;
mod session;
mod state;
mod theme;
mod views;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(|cx| {
            gpui_kit::init(cx);
            theme::init(cx);
            runtime::init(cx);
            state::AppState::init(cx);
            images::ImageStore::init(cx);

            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(900.)), cx)),
                window_min_size: Some(size(px(960.), px(600.))),
                window_decorations: Some(WindowDecorations::Client),
                window_background: WindowBackgroundAppearance::Transparent,
                app_id: Some("dev.aurelia.Aurelia".into()),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                cx.new(|cx| views::app::AppRoot::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
