#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

use gpui::{App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use std::path::PathBuf;
use ui::{Args, Lupasta, window_state};

fn parse_args() -> Args {
    let mut args = Args { root: None, select: None, data_dir: None, no_index: false, no_gitignore: false, smoke: false };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => args.root = it.next().map(PathBuf::from),
            "--select" => args.select = it.next(),
            "--data-dir" => args.data_dir = it.next().map(PathBuf::from),
            "--no-index" => args.no_index = true,
            "--no-gitignore" => args.no_gitignore = true,
            "--smoke" => args.smoke = true,
            _ => {}
        }
    }
    args
}

fn main() {
    let args = parse_args();
    gpui_platform::application().run(move |cx: &mut App| {
        ui::load_bundled(cx);
        // Smoke runs get the default window, not the one left last time.
        let data_dir = args.data_dir.clone().unwrap_or_else(ui::default_data_dir);
        let remember = !args.smoke;
        let saved = if remember { window_state::load(&data_dir, cx) } else { None };
        let options = WindowOptions {
            window_bounds: Some(saved.unwrap_or_else(|| WindowBounds::Windowed(Bounds::centered(None, size(px(906.0), px(672.0)), cx)))),
            titlebar: Some(TitlebarOptions { title: Some("lupasta".into()), ..Default::default() }),
            app_id: Some("lupasta".into()),
            window_min_size: Some(size(px(320.0), px(200.0))),
            ..Default::default()
        };
        let mut args = Some(args);
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| match Lupasta::new(args.take().unwrap(), window, cx) {
                Ok(app) => app,
                Err(e) => {
                    eprintln!("lupasta: {e}");
                    std::process::exit(1);
                }
            })
        });
        let handle = match opened {
            Ok(h) => h,
            Err(e) => {
                eprintln!("lupasta: could not open a window: {e}");
                std::process::exit(1);
            }
        };
        if remember {
            let _ = handle.update(cx, |_, window, cx| {
                window.on_window_should_close(cx, move |window, _| {
                    window_state::save(&data_dir, window.window_bounds());
                    true
                });
            });
        }
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
