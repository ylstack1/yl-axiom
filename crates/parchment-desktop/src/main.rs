#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! YL Axiom v3 Native Windows Desktop Entry Point (GPUI + `parchment-core` + Win32 DWM/COM).
//!
//! Default data root:
//! - Development / debug: `./parchment-portable-data` (never touches installed user data)
//! - Release / packaged: `%LOCALAPPDATA%\ParchmentAI` (overridable via `--data-dir` / `--user-dir`)

mod app;
mod composer;
mod icons;
mod ota;
mod overlays;
mod prose_view;
mod settings_view;
mod shortcuts;
mod sidebar;
mod strings;
mod theme;
mod ui_kit;
mod vault;
mod win32;

use std::path::PathBuf;
use std::time::Instant;

use gpui::{
    App, AppContext as _, Bounds, DisplayId, Pixels, WindowBounds, WindowOptions, bounds, point,
    px, size,
};
use gpui_component::Root;
use parchment_core::ParchmentCore;
use parchment_core::model::DesktopWindowConfig;

use crate::app::ParchmentDesktopApp;
use crate::theme::{
    ParchmentPalette, register_primary_fonts, register_secondary_fonts, sync_gpui_component_theme,
};
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
                    if let Some(val) = args.next() {
                        data_dir = PathBuf::from(val);
                    }
                }
                "--seed" => seed = true,
                "--bench-stream" => bench_stream = true,
                "--bench-out" => {
                    if let Some(val) = args.next() {
                        bench_out = Some(PathBuf::from(val));
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
            return PathBuf::from(local).join("YL-Axiom");
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

    let mut migration_warning: Option<String> = None;
    if cli.seed {
        core.seed_benchmark_dataset(300, 40, 200)?;
    } else if !cli.bench_stream
        && let Err(err) = core.migrate_v2_data_dir(&cli.data_dir, false)
    {
        eprintln!(
            "parchment-desktop: v2 migration failed for {}: {err}",
            cli.data_dir.display()
        );
        migration_warning = Some(format!(
            "Could not finish migrating previous YL Axiom v2 data: {err}"
        ));
    }

    let mut snapshot = core.load_settings_snapshot().unwrap_or_default();
    let win_cfg = core.load_desktop_window().unwrap_or_default();
    snapshot.desktop_window = win_cfg.clone();
    let stubs = core.list_chat_stubs()?;
    let startup_core_ms = t_start.elapsed().as_secs_f64() * 1000.0;

    let initial_palette = ParchmentPalette::from_config(&snapshot.theme);
    let data_dir = cli.data_dir;
    let bench_stream = cli.bench_stream;
    let bench_out = cli.bench_out;
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_component::init(cx);
        register_primary_fonts(cx);
        sync_gpui_component_theme(&initial_palette, cx);
        cx.set_reduce_motion(snapshot.preferences.reduce_motion);
        let (win_bounds, display_id) = resolve_initial_window_bounds(&win_cfg, bench_stream, cx);

        if let Err(err) = cx.open_window(
            WindowOptions {
                window_bounds: Some(win_bounds),
                display_id,
                window_min_size: Some(size(px(480.0), px(360.0))),
                ..WindowOptions::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| {
                    ParchmentDesktopApp::new(
                        core,
                        data_dir,
                        snapshot,
                        stubs,
                        startup_core_ms,
                        bench_stream,
                        bench_out,
                        migration_warning,
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        ) {
            eprintln!("parchment-desktop: open_window failed: {err:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
        cx.spawn(async move |cx| {
            cx.update(|cx| register_secondary_fonts(cx));
        })
        .detach();
    });
    Ok(())
}

/// Checks whether a saved floating window's top 40px title strip overlaps any connected display
/// by at least 100×40 logical pixels (mirrors Kotlin `Main.kt::savedPositionIsVisible`).
fn saved_position_is_visible(
    x: f32,
    y: f32,
    width: f32,
    display_bounds: &[(f32, f32, f32, f32)],
) -> Option<usize> {
    let strip_left = x;
    let strip_top = y;
    let strip_right = x + width.max(1.0);
    let strip_bottom = y + 40.0;
    display_bounds.iter().position(|&(dx, dy, dw, dh)| {
        let overlap_w = strip_right.min(dx + dw) - strip_left.max(dx);
        let overlap_h = strip_bottom.min(dy + dh) - strip_top.max(dy);
        overlap_w >= 100.0 && overlap_h >= 40.0
    })
}

fn centered_bounds_in(area: Bounds<Pixels>, width: f32, height: f32) -> Bounds<Pixels> {
    let avail_w = f32::from(area.size.width).max(480.0);
    let avail_h = f32::from(area.size.height).max(360.0);
    let w = width.min((avail_w - 32.0).max(480.0));
    let h = height.min((avail_h - 32.0).max(360.0));
    let x = f32::from(area.origin.x) + ((avail_w - w) * 0.5).max(0.0);
    let y = f32::from(area.origin.y) + ((avail_h - h) * 0.5).max(0.0);
    bounds(point(px(x), px(y)), size(px(w), px(h)))
}

fn resolve_initial_window_bounds(
    win_cfg: &DesktopWindowConfig,
    bench_stream: bool,
    cx: &App,
) -> (WindowBounds, Option<DisplayId>) {
    let w = win_cfg.width_dp.clamp(480, 7680) as f32;
    let h = win_cfg.height_dp.clamp(360, 4320) as f32;
    let displays = cx.displays();
    let display_rects: Vec<(f32, f32, f32, f32)> = displays
        .iter()
        .map(|d| {
            let b = d.visible_bounds();
            (
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            )
        })
        .collect();

    let has_saved_pos = !(win_cfg.x_dp == -1 && win_cfg.y_dp == -1);
    let (restored_rect, display_id) = if has_saved_pos
        && let Some(idx) =
            saved_position_is_visible(win_cfg.x_dp as f32, win_cfg.y_dp as f32, w, &display_rects)
    {
        (
            bounds(
                point(px(win_cfg.x_dp as f32), px(win_cfg.y_dp as f32)),
                size(px(w), px(h)),
            ),
            Some(displays[idx].id()),
        )
    } else if let Some(primary) = cx.primary_display().or_else(|| displays.first().cloned()) {
        (
            centered_bounds_in(primary.visible_bounds(), w, h),
            Some(primary.id()),
        )
    } else {
        (bounds(point(px(80.0), px(60.0)), size(px(w), px(h))), None)
    };

    let wb = if win_cfg.is_maximized && !bench_stream {
        WindowBounds::Maximized(restored_rect)
    } else {
        WindowBounds::Windowed(restored_rect)
    };
    (wb, display_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_position_visibility_handles_multimonitor_and_unplugged_screens() {
        let screens = [(0.0, 0.0, 1920.0, 1040.0), (-1280.0, 0.0, 1280.0, 720.0)];
        assert_eq!(
            saved_position_is_visible(120.0, 90.0, 1000.0, &screens),
            Some(0)
        );
        // Left secondary monitor with negative X coordinate.
        assert_eq!(
            saved_position_is_visible(-1100.0, 80.0, 900.0, &screens),
            Some(1)
        );
        // Unplugged monitor far to the right returns None so startup centers cleanly.
        assert_eq!(
            saved_position_is_visible(3200.0, 100.0, 900.0, &screens),
            None
        );
        // Minimized Win32 off-screen coordinate is rejected.
        assert_eq!(
            saved_position_is_visible(-32000.0, -32000.0, 900.0, &screens),
            None
        );
    }
}
