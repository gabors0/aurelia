mod components;
mod format;
mod images;
mod loadable;
mod nav;
mod runtime;
mod session;
mod state;
mod theme;

use gpui_kit::component::{TitleBar, h_flex, v_flex};
use gpui_kit::*;

use crate::theme::Palette;

struct Aurelia;

impl Render for Aurelia {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .bg(Palette::bg())
            .text_color(Palette::text())
            .child(
                TitleBar::new().h(px(52.)).child(
                    h_flex()
                        .gap_2()
                        .child(div().size_3().rounded_full().bg(Palette::accent()))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child("Aurelia")),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .font_family(theme::FONT_DISPLAY)
                            .text_size(px(56.))
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child("Aurelia"),
                    )
                    .child(
                        div()
                            .text_color(Palette::text_secondary())
                            .child("A cinematic Jellyfin client"),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
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
            gpui_kit::open_window(options, cx, |_, cx| cx.new(|_| Aurelia))
                .expect("failed to open window");
            cx.activate(true);
        });
}
