use crate::bubble_window::{clamp_top_left, LogicalPoint, LogicalRect, LogicalSize, BUBBLE_SIZE};
use crate::error::{AppError, Result};
use crate::settings;
use crate::state::app_data_dir;
use tauri::{Manager, PhysicalPosition, Position, WebviewWindow};

pub(crate) const BUBBLE_WINDOW_LABEL: &str = "recorder-bubble";

fn window_error(context: &str, error: impl std::fmt::Display) -> AppError {
    AppError::Io(format!("{context}: {error}"))
}

pub(crate) fn setup_bubble(app: &mut tauri::App) -> Result<()> {
    let persisted = settings::get_settings(&app_data_dir(app.handle())?);
    let bubble = tauri::WebviewWindowBuilder::new(
        app,
        BUBBLE_WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html?window=recorder-bubble".into()),
    )
    .title("WhisperPilot status")
    .inner_size(BUBBLE_SIZE, BUBBLE_SIZE)
    .min_inner_size(BUBBLE_SIZE, BUBBLE_SIZE)
    .max_inner_size(BUBBLE_SIZE, BUBBLE_SIZE)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .focused(false)
    .visible(false)
    .skip_taskbar(true)
    .always_on_top(persisted.bubble_always_on_top)
    .visible_on_all_workspaces(persisted.bubble_always_on_top)
    .build()
    .map_err(|error| window_error("could not create Recorder bubble", error))?;

    if let (Some(x), Some(y)) = (persisted.bubble_x, persisted.bubble_y) {
        let _ = bubble.set_position(Position::Logical(tauri::LogicalPosition::new(x, y)));
    }

    let moved_window = bubble.clone();
    let moved_app = app.handle().clone();
    bubble.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(position) = event {
            let scale = moved_window.scale_factor().unwrap_or(1.0);
            let logical = position.to_logical::<f64>(scale);
            if let Ok(dir) = app_data_dir(&moved_app) {
                let _ = settings::set_bubble_position(&dir, logical.x, logical.y);
            }
        }
    });
    Ok(())
}

fn windows(app: &tauri::AppHandle) -> Result<(WebviewWindow, WebviewWindow)> {
    let main = app
        .get_webview_window("main")
        .ok_or_else(|| AppError::Io("main window is unavailable".into()))?;
    let bubble = app
        .get_webview_window(BUBBLE_WINDOW_LABEL)
        .ok_or_else(|| AppError::Io("Recorder bubble window is unavailable".into()))?;
    Ok((main, bubble))
}

#[tauri::command]
pub(crate) fn collapse_to_bubble(app: tauri::AppHandle) -> Result<()> {
    let (main, bubble) = windows(&app)?;
    let main_position = main
        .outer_position()
        .map_err(|error| window_error("could not read main-window position", error))?;
    bubble
        .set_position(Position::Physical(main_position))
        .map_err(|error| window_error("could not position Recorder bubble", error))?;
    if let Err(error) = bubble.show() {
        let _ = main.show();
        return Err(window_error("could not show Recorder bubble", error));
    }
    if let Err(error) = main.hide() {
        let _ = bubble.hide();
        let _ = main.show();
        return Err(window_error("could not hide main window", error));
    }
    Ok(())
}

fn display_rectangles(main: &WebviewWindow) -> Result<Vec<LogicalRect>> {
    let mut monitors = main
        .available_monitors()
        .map_err(|error| window_error("could not enumerate displays", error))?;
    if let Some(primary) = main
        .primary_monitor()
        .map_err(|error| window_error("could not read primary display", error))?
    {
        monitors.sort_by_key(|monitor| monitor.position() != primary.position());
    }
    Ok(monitors
        .iter()
        .map(|monitor| {
            let work_area = monitor.work_area();
            LogicalRect::new(
                work_area.position.x as f64,
                work_area.position.y as f64,
                work_area.size.width as f64,
                work_area.size.height as f64,
            )
        })
        .collect())
}

#[tauri::command]
pub(crate) fn restore_main_from_bubble(app: tauri::AppHandle) -> Result<()> {
    let (main, bubble) = windows(&app)?;
    let bubble_position = bubble
        .outer_position()
        .map_err(|error| window_error("could not read Recorder bubble position", error))?;
    let main_size = main
        .outer_size()
        .map_err(|error| window_error("could not read main-window size", error))?;
    let clamped = clamp_top_left(
        LogicalPoint::new(bubble_position.x as f64, bubble_position.y as f64),
        LogicalSize::new(main_size.width as f64, main_size.height as f64),
        &display_rectangles(&main)?,
    );
    let target = PhysicalPosition::new(clamped.x.round() as i32, clamped.y.round() as i32);

    if let Err(error) = main.set_position(Position::Physical(target)) {
        let _ = main.show();
        return Err(window_error(
            "could not restore main-window position",
            error,
        ));
    }
    if let Err(error) = main.show() {
        return Err(window_error("could not show main window", error));
    }
    if let Err(error) = main.set_focus() {
        let _ = bubble.show();
        return Err(window_error("could not focus main window", error));
    }
    bubble
        .hide()
        .map_err(|error| window_error("could not hide Recorder bubble", error))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn set_bubble_always_on_top(
    app: tauri::AppHandle,
    value: bool,
) -> Result<settings::Settings> {
    let (_, bubble) = windows(&app)?;
    let dir = app_data_dir(&app)?;
    let previous = settings::get_settings(&dir).bubble_always_on_top;
    bubble
        .set_always_on_top(value)
        .map_err(|error| window_error("could not change Recorder bubble level", error))?;
    if let Err(error) = bubble.set_visible_on_all_workspaces(value) {
        let _ = bubble.set_always_on_top(previous);
        return Err(window_error(
            "could not change Recorder bubble workspace visibility",
            error,
        ));
    }
    match settings::set_setting(
        &dir,
        "bubble_always_on_top",
        if value { "true" } else { "false" },
    ) {
        Ok(settings) => Ok(settings),
        Err(error) => {
            let _ = bubble.set_always_on_top(previous);
            let _ = bubble.set_visible_on_all_workspaces(previous);
            Err(error)
        }
    }
}
