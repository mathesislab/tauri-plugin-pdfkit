//! A PDF in a native view of PDFKit, as Preview shows it (macOS). The view
//! lies in a web view, over an area of its page, which tells where that area
//! is and drives the view: find, zoom, the outline; the view tells it of the
//! page in view. One view per web view: a window may hold several.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
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
use tauri::{Emitter, EventTarget, Runtime, Webview};

use crate::{Entry, Frame, Opened, PAGE_EVENT};

/// The PDF shown in a web view, with what tells the page of the page in view.
struct Shown {
    id: String,
    /// the label of the window of the web view
    window: String,
    view: Retained<PDFView>,
    document: Retained<PDFDocument>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

thread_local! {
    /// the view of each web view, by its label; made and used on the main
    /// thread only, as AppKit wants
    static SHOWN: RefCell<HashMap<String, Shown>> = RefCell::new(HashMap::new());
}

/// Run `f` on the main thread, and wait for its answer.
fn on_main<R: Runtime, T: Send + 'static>(
    webview: &Webview<R>,
    f: impl FnOnce(MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    if let Some(mtm) = MainThreadMarker::new() {
        return Ok(f(mtm));
    }
    let (tx, rx) = mpsc::channel();
    webview
        .run_on_main_thread(move || {
            let _ = tx.send(f(MainThreadMarker::new().unwrap()));
        })
        .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())
}

/// Open `path` at `page` (from 1), over `frame`, in place of the PDF the
/// web view shows; hidden when a modal of the page is `covered`. `zoom`: the
/// points of a CSS pixel.
pub fn open<R: Runtime>(
    webview: Webview<R>,
    id: String,
    path: PathBuf,
    page: usize,
    frame: Frame,
    covered: bool,
    zoom: f64,
) -> Result<Opened, String> {
    if !path.exists() {
        return Err(format!("{}: not found", path.display()));
    }
    // the web view, in whose coordinates the page gives the area; a pointer
    // is not Send: its address goes to the main thread
    let (tx, rx) = mpsc::channel();
    webview
        .with_webview(move |platform| {
            let _ = tx.send(platform.inner() as usize);
        })
        .map_err(|e| e.to_string())?;
    let address = rx.recv().map_err(|e| e.to_string())?;
    let label = webview.label().to_string();
    let window = webview.window().label().to_string();
    let handle = webview.clone();
    on_main(&webview, move |mtm| {
        remove(&label);
        // the web view lives while its page asks
        let content = unsafe { &*(address as *const NSView) };
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let document = unsafe { PDFDocument::initWithURL(PDFDocument::alloc(), &url) }
            .ok_or_else(|| format!("{}: not a PDF that PDFKit reads", path.display()))?;
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
        view.setHidden(covered);
        // the keys scroll the PDF at once
        if !covered && let Some(window) = view.window() {
            window.makeFirstResponder(Some(&view));
        }
        let observer = observe(handle, &view, &document);
        let opened = Opened {
            count: unsafe { document.pageCount() },
            outline: unsafe { document.outlineRoot() }
                .map(|root| entries(&root, &mut Vec::new()))
                .unwrap_or_default(),
        };
        SHOWN.with_borrow_mut(|shown| {
            shown.insert(
                label,
                Shown {
                    id,
                    window,
                    view,
                    document,
                    observer,
                },
            )
        });
        Ok(opened)
    })?
}

/// The page in view, told to the page of the web view as it changes (the
/// notification comes on the main thread, where the view is).
fn observe<R: Runtime>(
    webview: Webview<R>,
    view: &Retained<PDFView>,
    document: &Retained<PDFDocument>,
) -> Retained<ProtocolObject<dyn NSObjectProtocol>> {
    let (shown, doc) = (view.clone(), document.clone());
    let target = EventTarget::webview(webview.label());
    let block = RcBlock::new(move |_: NonNull<NSNotification>| {
        if let Some(page) = unsafe { shown.currentPage() } {
            let index = unsafe { doc.indexForPage(&page) };
            let _ = webview.emit_to(target.clone(), PAGE_EVENT, index + 1);
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

/// The area of the web view's PDF moved or changed size.
pub fn set_frame<R: Runtime>(webview: Webview<R>, frame: Frame, zoom: f64) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            if let Some(Shown { view, .. }) = shown.get(&label)
                && let Some(content) = unsafe { view.superview() }
            {
                view.setFrame(rect(&content, frame, zoom));
            }
        });
    })
}

/// A modal of the page opened (`covered`) or closed: the view, which lies above all of the page, hides
/// so that the modal shows and takes the keys, and is shown again, with the keys, when it is closed.
pub fn cover<R: Runtime>(webview: Webview<R>, id: String, covered: bool) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            let Some(Shown { view, .. }) = shown.get(&label).filter(|s| s.id == id) else {
                return;
            };
            view.setHidden(covered);
            let Some(window) = view.window() else { return };
            if covered {
                // the keys go to the page, to the modal
                if let Some(page) = unsafe { view.superview() } {
                    window.makeFirstResponder(Some(&page));
                }
            } else {
                window.makeFirstResponder(Some(view));
            }
        });
    })
}

/// The PDF `id` is not in front any more: its view goes.
pub fn close<R: Runtime>(webview: Webview<R>, id: String) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        if SHOWN.with_borrow(|shown| shown.get(&label).is_some_and(|s| s.id == id)) {
            remove(&label);
        }
    })
}

/// The window `label` is gone: the views of its web views go with it.
pub fn destroyed(label: &str) {
    if MainThreadMarker::new().is_some() {
        let gone: Vec<String> = SHOWN.with_borrow(|shown| {
            shown
                .iter()
                .filter(|(_, s)| s.window == label)
                .map(|(webview, _)| webview.clone())
                .collect()
        });
        for webview in gone {
            remove(&webview);
        }
    }
}

/// The page of a web view loads (again): its view goes, as nothing on the page keeps it in place any more.
pub fn unload<R: Runtime>(webview: &Webview<R>) {
    let label = webview.label().to_string();
    if MainThreadMarker::new().is_some() {
        remove(&label);
    } else {
        let _ = webview.run_on_main_thread(move || remove(&label));
    }
}

/// The view of the web view `label` taken away, if one is shown (on the main thread).
fn remove(label: &str) {
    if let Some(shown) = SHOWN.with_borrow_mut(|shown| shown.remove(label)) {
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
pub fn find<R: Runtime>(
    webview: Webview<R>,
    text: String,
    again: bool,
    backwards: bool,
) -> Result<bool, String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            let Some(Shown { view, document, .. }) = shown.get(&label) else {
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

/// In (1) or out (-1), or the width of the area (0).
pub fn zoom<R: Runtime>(webview: Webview<R>, step: i32) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            if let Some(Shown { view, .. }) = shown.get(&label) {
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
pub fn go<R: Runtime>(webview: Webview<R>, target: Vec<usize>) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            let Some(Shown { view, document, .. }) = shown.get(&label) else {
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
