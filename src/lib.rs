//! A PDF in a native view of PDFKit, as Preview shows it, over an area of
//! the page of a Tauri web view (macOS), one for each web view. Elsewhere the commands fail: the page
//! shows the PDF itself (pdf.js).
//!
//! ```ignore
//! tauri::Builder::default()
//!     .plugin(tauri_plugin_pdfkit::Builder::new().zoom(|app| text_zoom(app)).build())
//! ```

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::plugin::{self, TauriPlugin};
use tauri::webview::PageLoadEvent;
use tauri::{AppHandle, Manager, RunEvent, Runtime, State, Webview, WindowEvent};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;
#[cfg(not(target_os = "macos"))]
mod other;
#[cfg(not(target_os = "macos"))]
use other as platform;

/// The event that tells the page of a web view the page in view (from 1).
pub const PAGE_EVENT: &str = "pdfkit:page";

/// The event that tells the page of a web view the history of the PDF (a
/// `History`), when it changes.
pub const HISTORY_EVENT: &str = "pdfkit:history";

/// Whether there is a page to go back to (the one a link or a jump left), or
/// forward (after going back).
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
pub struct History {
    pub back: bool,
    pub forward: bool,
}

/// Where to turn from the page in view.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Turn {
    First,
    Previous,
    Next,
    Last,
}

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

/// A rectangle of a page, in page points, the origin at the bottom left as
/// PDFKit has it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A word of a page and the centre of its bounds (`x`, page points).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Run {
    pub text: String,
    pub x: f64,
}

/// A line of the selection: its `page` (from 1) and printed `label` (empty if
/// none), the part selected (`text`, in `bounds`), and the words on the same
/// band of the page outside it (`margin`), the line numbers printed beside the
/// text among them.
#[derive(Debug, Serialize)]
pub struct Line {
    pub page: usize,
    pub label: String,
    pub text: String,
    pub bounds: Rect,
    pub margin: Vec<Run>,
}

/// The text selected in the PDF, line by line.
#[derive(Debug, Serialize)]
pub struct Selection {
    pub text: String,
    pub lines: Vec<Line>,
}

type Factor<R> = Box<dyn Fn(&AppHandle<R>) -> f64 + Send + Sync>;

/// The points of a CSS pixel.
struct Zoom<R: Runtime>(Factor<R>);

impl<R: Runtime> Zoom<R> {
    fn factor(&self, webview: &Webview<R>) -> f64 {
        (self.0)(webview.app_handle())
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
                open, frame, cover, close, find, zoom, go, back, forward, page_to, turn, selection
            ])
            .setup(move |app, _| {
                app.manage(factor);
                Ok(())
            })
            // a page that loads again (a reload, a crash of its content) no longer keeps the view of the one before
            // in place: the view goes
            .on_page_load(|webview, payload| {
                if matches!(payload.event(), PageLoadEvent::Started) {
                    platform::unload(webview);
                }
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
/// web view shows; hidden while a modal of the page is `covered`. `id` names it
/// for `cover` and `close`.
#[tauri::command]
async fn open<R: Runtime>(
    webview: Webview<R>,
    zoom: State<'_, Zoom<R>>,
    id: String,
    path: PathBuf,
    page: usize,
    frame: Frame,
    covered: bool,
) -> Result<Opened, String> {
    let zoom = zoom.factor(&webview);
    platform::open(webview, id, path, page, frame, covered, zoom)
}

/// The area moved or changed size.
#[tauri::command]
async fn frame<R: Runtime>(
    webview: Webview<R>,
    zoom: State<'_, Zoom<R>>,
    frame: Frame,
) -> Result<(), String> {
    let zoom = zoom.factor(&webview);
    platform::set_frame(webview, frame, zoom)
}

/// A modal of the page opened (`covered`) or closed: the view, which lies
/// above all of the page, hides so that the modal shows and takes the keys.
#[tauri::command]
async fn cover<R: Runtime>(webview: Webview<R>, id: String, covered: bool) -> Result<(), String> {
    platform::cover(webview, id, covered)
}

/// The PDF `id` is not in front any more: its view goes.
#[tauri::command]
async fn close<R: Runtime>(webview: Webview<R>, id: String) -> Result<(), String> {
    platform::close(webview, id)
}

/// Find `text`: the first place, or `again` the next (the one before,
/// `backwards`). Whether found; an empty text clears.
#[tauri::command]
async fn find<R: Runtime>(
    webview: Webview<R>,
    text: String,
    again: bool,
    backwards: bool,
) -> Result<bool, String> {
    platform::find(webview, text, again, backwards)
}

/// In (1) or out (-1), or the width of the area (0).
#[tauri::command]
async fn zoom<R: Runtime>(webview: Webview<R>, step: i32) -> Result<(), String> {
    platform::zoom(webview, step)
}

/// Go to an entry of the outline, by its indices from the root.
#[tauri::command]
async fn go<R: Runtime>(webview: Webview<R>, target: Vec<usize>) -> Result<(), String> {
    platform::go(webview, target)
}

/// Back to the page the last link or jump left (see `History`).
#[tauri::command]
async fn back<R: Runtime>(webview: Webview<R>) -> Result<(), String> {
    platform::back(webview, false)
}

/// Forward again, after going back.
#[tauri::command]
async fn forward<R: Runtime>(webview: Webview<R>) -> Result<(), String> {
    platform::back(webview, true)
}

/// Go to `page` (from 1; the last, if beyond it).
#[tauri::command]
async fn page_to<R: Runtime>(webview: Webview<R>, page: usize) -> Result<(), String> {
    platform::page_to(webview, page)
}

/// Turn to the first, previous, next or last page.
#[tauri::command]
async fn turn<R: Runtime>(webview: Webview<R>, to: Turn) -> Result<(), String> {
    platform::turn(webview, to)
}

/// The text selected in the PDF, line by line, with the words beside each line
/// (the line numbers of a manuscript among them); none if nothing is selected.
#[tauri::command]
async fn selection<R: Runtime>(webview: Webview<R>) -> Result<Option<Selection>, String> {
    platform::selection(webview)
}
