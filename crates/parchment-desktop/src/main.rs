#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Parchment v3 Native Windows Desktop Entry Point (GPUI + `parchment-core` + Win32 DWM/COM).
//!
//! Default data root:
//! - Development / debug: `./parchment-portable-data` (never touches installed user data)
//! - Release / packaged: `%LOCALAPPDATA%\ParchmentAI` (overridable via `--data-dir` / `--user-dir`)

mod app;
mod ota;
mod prose_view;
mod settings_view;
mod strings;
mod theme;
mod win32;

use std::path::PathBuf;
use std::time::Instant;

use gpui::{App, AppContext as _, WindowBounds, WindowOptions, bounds, point, px, size};
use gpui_component::Root;
use parchment_core::ParchmentCore;

use crate::app::ParchmentDesktopApp;
use crate::prose_view::DisplayMessageRow;

struct CliArgs {
    data_dir: PathBuf,
    seed: bool,
    bench_stream: bool,
    bench_out: Option<PathBuf>,
}

impl CliArgs {
    fn parse() -> Self {
        let mut data_dir = default_data_dir();
        let mut seed = false;
        let mut bench_stream = false;
        let mut bench_out = None;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--data-dir" | "--user-dir" => {
                    if let Some(p) = args.next() {
                        data_dir = PathBuf::from(p);
                    }
                }
                "--seed" => seed = true,
                "--bench-stream" => bench_stream = true,
                "--bench-out" => {
                    if let Some(p) = args.next() {
                        bench_out = Some(PathBuf::from(p));
                    }
                }
                _ => {}
            }
        }
        Self {
            data_dir,
            seed,
            bench_stream,
            bench_out,
        }
    }
}

fn default_data_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    {
        PathBuf::from("parchment-portable-data")
    }
    #[cfg(not(debug_assertions))]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA")
            && !local.trim().is_empty()
        {
            return PathBuf::from(local).join("ParchmentAI");
        }
        PathBuf::from("parchment-portable-data")
    }
}

fn main() -> anyhow::Result<()> {
    let t_start = Instant::now();
    let cli = CliArgs::parse();
    std::fs::create_dir_all(&cli.data_dir)?;
    let db_path = cli.data_dir.join("parchment.db");
    let core = ParchmentCore::open(&db_path)?;

    if cli.seed {
        core.seed_benchmark_dataset(300, 40, 200)?;
    }
    let stubs = core.list_chat_stubs()?;
    let active_id = stubs.first().map(|s| s.id.clone()).unwrap_or_default();
    let initial_rows: Vec<DisplayMessageRow> = if active_id.is_empty() {
        Vec::new()
    } else {
        core.load_messages_window(&active_id, 200, 0)?
            .into_iter()
            .map(DisplayMessageRow::from_node)
            .collect()
    };
    let startup_core_ms = t_start.elapsed().as_secs_f64() * 1000.0;

    let data_dir = cli.data_dir;
    let bench_stream = cli.bench_stream;
    let bench_out = cli.bench_out;

    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        let win_bounds = bounds(point(px(80.), px(60.)), size(px(1240.), px(840.)));
        let _ = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(win_bounds)),
                ..WindowOptions::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| {
                    ParchmentDesktopApp::new(
                        core,
                        data_dir,
                        stubs,
                        initial_rows,
                        startup_core_ms,
                        bench_stream,
                        bench_out,
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        cx.activate(true);
    });

    Ok(())
}
