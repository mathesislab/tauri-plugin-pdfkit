//! A PDF in a native view of PDFKit, as Preview shows it, over an area of
//! the page of a Tauri window (macOS). Elsewhere the commands fail: the page
//! shows the PDF itself (pdf.js).
//!
//! ```ignore
//! tauri::Builder::default()
//!     .plugin(tauri_plugin_pdfkit::Builder::new().zoom(|app| text_zoom(app)).build())
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::plugin::{self, TauriPlugin};
use tauri::{AppHandle, Manager, RunEvent, Runtime, State, WebviewWindow, WindowEvent};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
use other as platform;

/// The event that tells the page of a window the page in view (from 1).
pub const PAGE_EVENT: &str = "pdfkit:page";

/// The area of the page where the PDF shows, in CSS pixels from the top left
/// of its viewport, and the height of the viewport: the web view may also lie
/// under the title bar, and the page start below it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub viewport: f64,
}

/// An entry of the outline; `target`, the indices that lead to it from the
/// root of the outline.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub title: String,
    pub target: Vec<usize>,
    pub items: Vec<Entry>,
}

/// What the page learns of a PDF opened.
#[derive(Debug, Serialize)]
pub struct Opened {
    pub count: usize,
    pub outline: Vec<Entry>,
}

type Factor<R> = Box<dyn Fn(&AppHandle<R>) -> f64 + Send + Sync>;

/// The points of a CSS pixel.
struct Zoom<R: Runtime>(Factor<R>);

impl<R: Runtime> Zoom<R> {
    fn factor(&self, window: &WebviewWindow<R>) -> f64 {
        (self.0)(window.app_handle())
    }
}

pub struct Builder<R: Runtime> {
    zoom: Factor<R>,
}

impl<R: Runtime> Default for Builder<R> {
    fn default() -> Self {
        Self {
            zoom: Box::new(|_| 1.0),
        }
    }
}

impl<R: Runtime> Builder<R> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The points of a CSS pixel, when the app zooms its pages (1 by default).
    pub fn zoom(mut self, factor: impl Fn(&AppHandle<R>) -> f64 + Send + Sync + 'static) -> Self {
        self.zoom = Box::new(factor);
        self
    }

    pub fn build(self) -> TauriPlugin<R> {
        // not named `zoom`, the command the handler calls
        let factor = Zoom(self.zoom);
        plugin::Builder::new("pdfkit")
            .invoke_handler(tauri::generate_handler![
                open, frame, cover, close, find, zoom, go
            ])
            .setup(move |app, _| {
                app.manage(factor);
                Ok(())
            })
            .on_event(|_, event| {
                if let RunEvent::WindowEvent {
                    label,
                    event: WindowEvent::Destroyed,
                    ..
                } = event
                {
                    platform::destroyed(label);
                }
            })
            .build()
    }
}

/// The plugin, the pages not zoomed.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new().build()
}

/// Open `path` at `page` (from 1), over `frame`, in place of the PDF the
/// window shows; hidden while a modal of the page is `covered`. `id` names it
/// for `cover` and `close`.
#[tauri::command]
async fn open<R: Runtime>(
    window: WebviewWindow<R>,
    zoom: State<'_, Zoom<R>>,
    id: String,
    path: PathBuf,
    page: usize,
    frame: Frame,
    covered: bool,
) -> Result<Opened, String> {
    let zoom = zoom.factor(&window);
    platform::open(window, id, path, page, frame, covered, zoom)
}

/// The area moved or changed size.
#[tauri::command]
async fn frame<R: Runtime>(
    window: WebviewWindow<R>,
    zoom: State<'_, Zoom<R>>,
    frame: Frame,
) -> Result<(), String> {
    let zoom = zoom.factor(&window);
    platform::set_frame(window, frame, zoom)
}

/// A modal of the page opened (`covered`) or closed: the view, which lies
/// above all of the page, hides so that the modal shows and takes the keys.
#[tauri::command]
async fn cover<R: Runtime>(window: WebviewWindow<R>, id: String, covered: bool) -> Result<(), String> {
    platform::cover(window, id, covered)
}

/// The PDF `id` is not in front any more: its view goes.
#[tauri::command]
async fn close<R: Runtime>(window: WebviewWindow<R>, id: String) -> Result<(), String> {
    platform::close(window, id)
}

/// Find `text`: the first place, or `again` the next (the one before,
/// `backwards`). Whether found; an empty text clears.
#[tauri::command]
async fn find<R: Runtime>(
    window: WebviewWindow<R>,
    text: String,
    again: bool,
    backwards: bool,
) -> Result<bool, String> {
    platform::find(window, text, again, backwards)
}

/// In (1) or out (-1), or the width of the area (0).
#[tauri::command]
async fn zoom<R: Runtime>(window: WebviewWindow<R>, step: i32) -> Result<(), String> {
    platform::zoom(window, step)
}

/// Go to an entry of the outline, by its indices from the root.
#[tauri::command]
async fn go<R: Runtime>(window: WebviewWindow<R>, target: Vec<usize>) -> Result<(), String> {
    platform::go(window, target)
}
