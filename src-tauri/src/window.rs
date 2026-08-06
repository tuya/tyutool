//! Main-window sizing and placement.
//!
//! The geometry is kept as free functions over plain `PhysicalRect` /
//! `PhysicalWindowSize` values so the arithmetic is unit-testable without a
//! running Tauri app; only `apply_default_main_window_layout` touches a real
//! window.

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition};

pub(crate) const DEFAULT_MAIN_WINDOW_WIDTH: f64 = 1280.0;
pub(crate) const DEFAULT_MAIN_WINDOW_HEIGHT: f64 = 800.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhysicalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhysicalWindowSize {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PhysicalWindowPosition {
    pub x: i32,
    pub y: i32,
}

/// Shrink `default` to fit `available`, never growing past it. No lower floor is
/// applied on purpose: the window's `minWidth`/`minHeight` in tauri.conf.json
/// already enforces one, and forcing a minimum here would push the window off a
/// work area smaller than that minimum.
fn fit_logical_dimension(default: f64, available: f64) -> f64 {
    if !available.is_finite() || available <= 0.0 {
        return default;
    }
    default.min(available)
}

fn default_main_window_logical_size(
    work_area: PhysicalRect,
    scale_factor: f64,
) -> LogicalSize<f64> {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    let available_width = f64::from(work_area.width) / scale_factor;
    let available_height = f64::from(work_area.height) / scale_factor;

    LogicalSize::new(
        fit_logical_dimension(DEFAULT_MAIN_WINDOW_WIDTH, available_width),
        fit_logical_dimension(DEFAULT_MAIN_WINDOW_HEIGHT, available_height),
    )
}

fn clamp_axis(position: i32, size: u32, work_start: i32, work_extent: u32) -> i32 {
    if size >= work_extent {
        return work_start;
    }

    let min = i64::from(work_start);
    let max = min + i64::from(work_extent) - i64::from(size);
    i64::from(position).clamp(min, max) as i32
}

fn clamp_outer_position_to_work_area(
    x: i32,
    y: i32,
    outer_size: PhysicalWindowSize,
    work_area: PhysicalRect,
) -> PhysicalWindowPosition {
    PhysicalWindowPosition {
        x: clamp_axis(x, outer_size.width, work_area.x, work_area.width),
        y: clamp_axis(y, outer_size.height, work_area.y, work_area.height),
    }
}

fn physical_rect_from_tauri(rect: &tauri::PhysicalRect<i32, u32>) -> PhysicalRect {
    PhysicalRect {
        x: rect.position.x,
        y: rect.position.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

/// Default main window size + safe visible placement (matches `tauri.conf.json` when it fits).
pub(crate) fn apply_default_main_window_layout(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("main") {
        let monitor = win
            .current_monitor()
            .map_err(|e| e.to_string())?
            .or_else(|| win.primary_monitor().ok().flatten())
            .or_else(|| {
                win.available_monitors()
                    .ok()
                    .and_then(|monitors| monitors.into_iter().next())
            });

        if let Some(monitor) = monitor {
            let work_area = physical_rect_from_tauri(monitor.work_area());
            let size = default_main_window_logical_size(work_area, monitor.scale_factor());
            win.set_size(size).map_err(|e| e.to_string())?;
            win.center().map_err(|e| e.to_string())?;

            let outer_position = win.outer_position().map_err(|e| e.to_string())?;
            let outer_size = win.outer_size().map_err(|e| e.to_string())?;
            let clamped = clamp_outer_position_to_work_area(
                outer_position.x,
                outer_position.y,
                PhysicalWindowSize {
                    width: outer_size.width,
                    height: outer_size.height,
                },
                work_area,
            );

            if clamped.x != outer_position.x || clamped.y != outer_position.y {
                win.set_position(PhysicalPosition::new(clamped.x, clamped.y))
                    .map_err(|e| e.to_string())?;
            }
        } else {
            win.set_size(LogicalSize::new(
                DEFAULT_MAIN_WINDOW_WIDTH,
                DEFAULT_MAIN_WINDOW_HEIGHT,
            ))
            .map_err(|e| e.to_string())?;
            win.center().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_shrinks_to_fit_high_dpi_work_area() {
        let work_area = PhysicalRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        };

        let size = default_main_window_logical_size(work_area, 1.5);

        assert_eq!(size.width, 1280.0);
        assert!(size.height < DEFAULT_MAIN_WINDOW_HEIGHT);
        assert!(size.height <= 1040.0 / 1.5);
    }

    #[test]
    fn clamp_outer_position_moves_window_below_work_area_top() {
        let work_area = PhysicalRect {
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
        };
        let outer_size = PhysicalWindowSize {
            width: 1200,
            height: 800,
        };

        let pos = clamp_outer_position_to_work_area(-100, -200, outer_size, work_area);

        assert_eq!(pos.x, 0);
        assert_eq!(pos.y, 40);
    }

    #[test]
    fn clamp_outer_position_keeps_title_bar_visible_when_window_is_taller_than_work_area() {
        let work_area = PhysicalRect {
            x: 100,
            y: 100,
            width: 800,
            height: 500,
        };
        let outer_size = PhysicalWindowSize {
            width: 900,
            height: 700,
        };

        let pos = clamp_outer_position_to_work_area(20, 20, outer_size, work_area);

        assert_eq!(pos.x, 100);
        assert_eq!(pos.y, 100);
    }
}
