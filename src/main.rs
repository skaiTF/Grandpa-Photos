mod app;
mod transfer;
mod ui;

use app::App;

fn main() -> iced::Result {
    tracing_subscriber::fmt::init();

    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1100.0, 800.0))
        .run()
}
