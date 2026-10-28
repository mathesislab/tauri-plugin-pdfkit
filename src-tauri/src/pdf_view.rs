//! The PDF of the paper in front of the main window, in a native view of
//! PDFKit, as Preview shows it (macOS). The view lies in the web view, over
//! the reader area of the page, which tells where that area is and drives the
//! view: find, zoom, the outline; the view tells it of the page in view.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSColor, NSView, NSWindowOrderingMode};
use objc2_foundation::{
    NSArray, NSEdgeInsets, NSNotification, NSNotificationCenter, NSPoint, NSRect, NSSize, NSString,
    NSStringCompareOptions, NSURL,
};
use objc2_pdf_kit::{
    PDFDisplayMode, PDFDocument, PDFOutline, PDFView, PDFViewPageChangedNotification,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::AppState;
use crate::main_window::MAIN;

/// The reader area of the page, in CSS pixels from the top left of its
/// viewport, and the height of the viewport: the web view also lies under
/// the title bar, and the page starts below it. The zoom of the text size
/// makes a CSS pixel larger than a point: `rect` multiplies by it.
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    viewport: f64,
}

/// An entry of the outline; `target`, the indices that lead to it from the
/// root of the outline.
#[derive(Debug, Serialize)]
pub struct Entry {
    title: String,
    target: Vec<usize>,
    items: Vec<Entry>,
}

/// What the page learns of a PDF opened.
#[derive(Debug, Serialize)]
pub struct Opened {
    count: usize,
    outline: Vec<Entry>,
}

/// The PDF shown, with what tells the page of the page in view.
struct Shown {
    id: String,
    view: Retained<PDFView>,
    document: Retained<PDFDocument>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

thread_local! {
    /// made and used on the main thread only, as AppKit wants
    static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
}

/// Run `f` on the main thread, and wait for its answer.
fn on_main<R: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(MainThreadMarker) -> R + Send + 'static,
) -> Result<R, String> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(mtm));
    }
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(MainThreadMarker::new().unwrap()));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

/// Open the PDF of item `id` at `page` (from 1), over `frame`, in place of
/// the one shown.
#[tauri::command]
pub async fn pdf_open(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    page: usize,
    frame: Frame,
) -> Result<Opened, String> {
    let item = state.item(&id)?;
    let file = item
        .files
        .into_iter()
        .find(|f| f.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        .ok_or(format!("{id}: no PDF"))?;
    if !file.exists() {
        return Err(format!("{}: not found", file.display()));
    }
    // the web view, in whose coordinates the page gives the area; a pointer
    // is not Send: its address goes to the main thread
    let window = app.get_webview_window(MAIN).ok_or("no main window")?;
    let (tx, rx) = mpsc::channel();
    window
        .with_webview(move |webview| {
            let _ = tx.send(webview.inner() as usize);
        })
        .map_err(|e| e.to_string())?;
    let webview = rx.recv().map_err(|e| e.to_string())?;
    let handle = app.clone();
    let zoom = crate::text_size::factor(&app);
    on_main(&app, move |mtm| {
        close(mtm);
        // the web view lives while its page asks
        let content = unsafe { &*(webview as *const NSView) };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
        let document = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }
            .ok_or_else(|| format!("{}: not a PDF that PDFKit reads", file.display()))?;
        let view = unsafe { PDFView::initWithFrame(PDFView::alloc(mtm), rect(content, frame, zoom)) };
        unsafe {
            view.setDisplayMode(PDFDisplayMode::SinglePageContinuous);
            // margins at the sides of the pages, as Preview leaves them
            view.setPageBreakMargins(NSEdgeInsets {
                top: 8.0,
                left: 24.0,
                bottom: 8.0,
                right: 24.0,
            });
            view.setAutoScales(true);
            view.setBackgroundColor(&NSColor::underPageBackgroundColor());
            view.setDocument(Some(&document));
            if page > 1
                && let Some(p) = document.pageAtIndex(page - 1)
            {
                view.goToPage(&p);
            }
        }
        content.addSubview_positioned_relativeTo(&view, NSWindowOrderingMode::Above, None);
        // the keys scroll the PDF at once
        if let Some(window) = view.window() {
            window.makeFirstResponder(Some(&view));
        }
        let observer = observe(handle, &view, &document);
        let opened = Opened {
            count: unsafe { document.pageCount() },
            outline: unsafe { document.outlineRoot() }
                .map(|root| entries(&root, &mut Vec::new()))
                .unwrap_or_default(),
        };
        SHOWN.set(Some(Shown {
            id,
            view,
            document,
            observer,
        }));
        Ok(opened)
    })?
}

/// The page in view, told to the page as it changes (the notification comes
/// on the main thread, where the view is).
fn observe(
    app: AppHandle,
    view: &Retained<PDFView>,
    document: &Retained<PDFDocument>,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let (shown, doc) = (view.clone(), document.clone());
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(page) = unsafe { shown.currentPage() } {
            let index = unsafe { doc.indexForPage(&page) };
            let _ = app.emit_to(MAIN, "pdf-page", index + 1);
        }
    });
    unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(PDFViewPageChangedNotification),
            Some(view),
            None,
            &block,
        )
    }
}

fn entries(outline: &PDFOutline, path: &mut Vec<usize>) -> Vec<Entry> {
    (0..unsafe { outline.numberOfChildren() })
        .filter_map(|i| {
            let child = unsafe { outline.childAtIndex(i) }?;
            path.push(i);
            let entry = Entry {
                title: unsafe { child.label() }
                    .map(|l| l.to_string())
                    .unwrap_or_default(),
                target: path.clone(),
                items: entries(&child, path),
            };
            path.pop();
            Some(entry)
        })
        .collect()
}

/// A frame of the page in the coordinates of the web view, whose origin may
/// be at the bottom; the viewport of the page is at its bottom. `zoom`: the
/// points of a CSS pixel.
fn rect(content: &NSView, frame: Frame, zoom: f64) -> NSRect {
    let frame = Frame {
        x: frame.x * zoom,
        y: frame.y * zoom,
        width: frame.width * zoom,
        height: frame.height * zoom,
        viewport: frame.viewport * zoom,
    };
    let height = content.bounds().size.height;
    let top = (height - frame.viewport).max(0.0);
    let y = if content.isFlipped() {
        top + frame.y
    } else {
        height - top - frame.y - frame.height
    };
    NSRect::new(
        NSPoint::new(frame.x, y),
        NSSize::new(frame.width, frame.height),
    )
}

/// The reader area moved or changed size.
#[tauri::command]
pub async fn pdf_frame(app: AppHandle, frame: Frame) -> Result<(), String> {
    let zoom = crate::text_size::factor(&app);
    on_main(&app, move |_| {
        SHOWN.with_borrow(|shown| {
            if let Some(Shown { view, .. }) = shown
                && let Some(content) = unsafe { view.superview() }
            {
                view.setFrame(rect(&content, frame, zoom));
            }
        });
    })
}

/// The paper `id` is not in front any more: its view goes.
#[tauri::command]
pub async fn pdf_close(app: AppHandle, id: String) -> Result<(), String> {
    on_main(&app, move |mtm| {
        if SHOWN.with_borrow(|shown| shown.as_ref().is_some_and(|s| s.id == id)) {
            close(mtm);
        }
    })
}

/// The view taken away, if one is shown: when another opens, or the main
/// window closes.
pub fn close(_: MainThreadMarker) {
    if let Some(shown) = SHOWN.take() {
        let observer: &AnyObject = (*shown.observer).as_ref();
        unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
        // the keys go back to the page
        let page = unsafe { shown.view.superview() };
        shown.view.removeFromSuperview();
        if let Some(page) = page
            && let Some(window) = page.window()
        {
            window.makeFirstResponder(Some(&page));
        }
    }
}

/// Find `text`, all its places marked: the first one, or `again` the next
/// (or the one before, `backwards`). Whether found; an empty text clears.
#[tauri::command]
pub async fn pdf_find(
    app: AppHandle,
    text: String,
    again: bool,
    backwards: bool,
) -> Result<bool, String> {
    on_main(&app, move |_| {
        SHOWN.with_borrow(|shown| {
            let Some(Shown { view, document, .. }) = shown else {
                return false;
            };
            unsafe {
                // PDFKit keeps drawing the marks it had: cleared first, the
                // page redrawn after
                view.setHighlightedSelections(None);
                if text.is_empty() {
                    view.clearSelection();
                    redraw(view);
                    return false;
                }
                let text = NSString::from_str(&text);
                let all: Retained<NSArray<_>> = document
                    .findString_withOptions(&text, NSStringCompareOptions::CaseInsensitiveSearch);
                // all marked as found text is, the one found selected
                let color = NSColor::findHighlightColor();
                for place in all.iter() {
                    place.setColor(Some(&color));
                }
                view.setHighlightedSelections(Some(&all));
                redraw(view);
                let mut options = NSStringCompareOptions::CaseInsensitiveSearch;
                if backwards {
                    options |= NSStringCompareOptions::BackwardsSearch;
                }
                let from = if again { view.currentSelection() } else { None };
                // from the end, round to the start
                let found = document
                    .findString_fromSelection_withOptions(&text, from.as_deref(), options)
                    .or_else(|| {
                        document.findString_fromSelection_withOptions(&text, None, options)
                    });
                match found {
                    Some(selection) => {
                        view.setCurrentSelection_animate(Some(&selection), true);
                        view.goToSelection(&selection);
                        true
                    }
                    None => false,
                }
            }
        })
    })
}

unsafe fn redraw(view: &PDFView) {
    if let Some(pages) = unsafe { view.documentView() } {
        pages.setNeedsDisplay(true);
    }
}

/// In (1) or out (-1), or the width of the pane (0).
#[tauri::command]
pub async fn pdf_zoom(app: AppHandle, step: i32) -> Result<(), String> {
    on_main(&app, move |_| {
        SHOWN.with_borrow(|shown| {
            if let Some(Shown { view, .. }) = shown {
                unsafe {
                    match step {
                        1.. => view.zoomIn(None),
                        ..0 => view.zoomOut(None),
                        0 => view.setAutoScales(true),
                    }
                }
            }
        });
    })
}

/// Go to an entry of the outline, by its indices from the root.
#[tauri::command]
pub async fn pdf_go(app: AppHandle, target: Vec<usize>) -> Result<(), String> {
    on_main(&app, move |_| {
        SHOWN.with_borrow(|shown| {
            let Some(Shown { view, document, .. }) = shown else {
                return;
            };
            unsafe {
                let mut entry = document.outlineRoot();
                for &i in &target {
                    entry = entry.and_then(|e| e.childAtIndex(i));
                }
                if let Some(destination) = entry.and_then(|e| e.destination()) {
                    view.goToDestination(&destination);
                }
            }
        });
    })
}
