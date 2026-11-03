//! A PDF in a native view of PDFKit, as Preview shows it (macOS). The view
//! lies in a web view, over an area of its page, which tells where that area
//! is and drives the view: find, zoom, the outline; the view tells it of the
//! page in view. One view per web view: a window may hold several.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::mpsc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSColor, NSView, NSWindowOrderingMode};
use objc2_foundation::{
    NSArray, NSEdgeInsets, NSNotification, NSNotificationCenter, NSPoint, NSRange, NSRect, NSSize,
    NSString, NSStringCompareOptions, NSURL,
};
use objc2_pdf_kit::{
    PDFDisplayMode, PDFDocument, PDFOutline, PDFPage, PDFSelection, PDFView,
    PDFViewChangedHistoryNotification, PDFViewPageChangedNotification,
};
use tauri::{Emitter, EventTarget, Runtime, Webview};

use crate::{
    Entry, Frame, HISTORY_EVENT, History, Line, Opened, PAGE_EVENT, Rect, Run, Selection, Turn,
};

/// The PDF shown in a web view, with what tells the page of the page in view
/// and of its history.
struct Shown {
    id: String,
    /// the label of the window of the web view
    window: String,
    view: Retained<PDFView>,
    document: Retained<PDFDocument>,
    observers: Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>>,
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
        let view =
            unsafe { PDFView::initWithFrame(PDFView::alloc(mtm), rect(content, frame, zoom)) };
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
        let observers = observe(handle, &view, &document);
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
                    observers,
                },
            )
        });
        Ok(opened)
    })?
}

/// The page in view and the history (whether there is a page to go back to,
/// or forward), told to the page of the web view as they change (the
/// notifications come on the main thread, where the view is).
fn observe<R: Runtime>(
    webview: Webview<R>,
    view: &Retained<PDFView>,
    document: &Retained<PDFDocument>,
) -> Vec<Retained<ProtocolObject<dyn NSObjectProtocol>>> {
    let target = EventTarget::webview(webview.label());
    // the history the page was told last: told again only when it changes
    let told = Rc::new(Cell::new(History::default()));
    let history = {
        let (webview, shown, target, told) =
            (webview.clone(), view.clone(), target.clone(), told.clone());
        move || {
            let now = History {
                back: unsafe { shown.canGoBack() },
                forward: unsafe { shown.canGoForward() },
            };
            if told.replace(now) != now {
                let _ = webview.emit_to(target.clone(), HISTORY_EVENT, now);
            }
        }
    };
    let on_page = {
        let (shown, doc, history) = (view.clone(), document.clone(), history.clone());
        RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(page) = unsafe { shown.currentPage() } {
                let index = unsafe { doc.indexForPage(&page) };
                let _ = webview.emit_to(target.clone(), PAGE_EVENT, index + 1);
            }
            // a link followed changes the page and the history together
            history();
        })
    };
    let on_history = RcBlock::new(move |_: NonNull<NSNotification>| history());
    let center = NSNotificationCenter::defaultCenter();
    unsafe {
        vec![
            center.addObserverForName_object_queue_usingBlock(
                Some(PDFViewPageChangedNotification),
                Some(view),
                None,
                &on_page,
            ),
            center.addObserverForName_object_queue_usingBlock(
                Some(PDFViewChangedHistoryNotification),
                Some(view),
                None,
                &on_history,
            ),
        ]
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
        for observer in &shown.observers {
            let observer: &AnyObject = (**observer).as_ref();
            unsafe { NSNotificationCenter::defaultCenter().removeObserver(observer) };
        }
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

/// Back to the page the last link or jump came from (`forward`: undone).
pub fn back<R: Runtime>(webview: Webview<R>, forward: bool) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            if let Some(Shown { view, .. }) = shown.get(&label) {
                unsafe {
                    if forward {
                        view.goForward(None)
                    } else {
                        view.goBack(None)
                    }
                }
            }
        });
    })
}

/// Go to `page` (from 1), kept to the pages there are.
pub fn page_to<R: Runtime>(webview: Webview<R>, page: usize) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            let Some(Shown { view, document, .. }) = shown.get(&label) else {
                return;
            };
            unsafe {
                let last = document.pageCount().saturating_sub(1);
                if let Some(page) = document.pageAtIndex(page.saturating_sub(1).min(last)) {
                    view.goToPage(&page);
                }
            }
        });
    })
}

/// Turn to the first, previous, next or last page.
pub fn turn<R: Runtime>(webview: Webview<R>, to: Turn) -> Result<(), String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            if let Some(Shown { view, .. }) = shown.get(&label) {
                unsafe {
                    match to {
                        Turn::First => view.goToFirstPage(None),
                        Turn::Previous => view.goToPreviousPage(None),
                        Turn::Next => view.goToNextPage(None),
                        Turn::Last => view.goToLastPage(None),
                    }
                }
            }
        });
    })
}

/// The text selected in the web view's PDF, line by line, with the words
/// beside each line; none if nothing is selected.
pub fn selection<R: Runtime>(webview: Webview<R>) -> Result<Option<Selection>, String> {
    let label = webview.label().to_string();
    on_main(&webview, move |_| {
        SHOWN.with_borrow(|shown| {
            let Shown { view, document, .. } = shown.get(&label)?;
            unsafe {
                let current = view.currentSelection()?;
                let text = current.string()?.to_string();
                if text.is_empty() {
                    return None;
                }
                Some(Selection {
                    text,
                    lines: lines(&current, document),
                })
            }
        })
    })
}

/// The lines of a selection, each on its first page; those without text are
/// left out.
unsafe fn lines(selection: &PDFSelection, document: &PDFDocument) -> Vec<Line> {
    // the text of each page, read once
    let mut pages: HashMap<usize, Text> = HashMap::new();
    unsafe { selection.selectionsByLine() }
        .iter()
        .filter_map(|line| unsafe {
            let page = line.pages().firstObject()?;
            let index = document.indexForPage(&page);
            let text = pages.entry(index).or_insert_with(|| Text::of(&page));
            let (string, bounds) = text.selected(&line, &page)?;
            Some(Line {
                page: index + 1,
                label: page.label().map(|l| l.to_string()).unwrap_or_default(),
                text: string,
                bounds,
                margin: text.margin(bounds),
            })
        })
        .collect()
}

/// A stretch of the string of a page without white space, in one piece on the
/// page: its range of the string, its bounds, and whether it is a number apart
/// from the text it is glued to in the string (a line number: PDFKit has it
/// at the end of its line, with no space before it).
struct Part {
    range: Range<usize>,
    bounds: Rect,
    apart: bool,
}

/// The string of a page (UTF-16 units, as NSString counts them) and its parts.
struct Text {
    units: Vec<u16>,
    parts: Vec<Part>,
}

/// The share of the height of a line left out of its band at the top and at the
/// bottom, so that the lines above and below do not bleed in.
const BAND_INSET: f64 = 0.2;

/// The gap between two characters, in heights of a character, from which
/// they are not in one piece.
const GAP: f64 = 0.5;

impl Text {
    /// The bounds come from the selections of ranges of the string:
    /// `characterBoundsAtIndex` counts the characters of the page in another
    /// way than its string and the selections do.
    unsafe fn of(page: &PDFPage) -> Self {
        let units: Vec<u16> = unsafe { page.string() }
            .map(|string| string.to_string().encode_utf16().collect())
            .unwrap_or_default();
        let bounds = |range: &Range<usize>| {
            let place = unsafe { page.selectionForRange(NSRange::new(range.start, range.len())) };
            place.map(|place| rect_of(unsafe { place.boundsForPage(page) }))
        };
        let mut parts = Vec::new();
        for word in ranges(&units) {
            // only a word of digits and other characters may have a number glued to it
            let pieces = if glued(&units[word.clone()]) {
                let chars: Vec<Rect> = word
                    .clone()
                    .map(|i| bounds(&(i..i + 1)).unwrap_or(EMPTY))
                    .collect();
                pieces(&units[word.clone()], &chars)
            } else {
                vec![(0..word.len(), false)]
            };
            for (piece, apart) in pieces {
                let range = word.start + piece.start..word.start + piece.end;
                if let Some(bounds) = bounds(&range) {
                    parts.push(Part {
                        range,
                        bounds,
                        apart,
                    });
                }
            }
        }
        Self { units, parts }
    }

    /// The text of a line of a selection and its bounds on the page, without a
    /// number glued to its end or start.
    unsafe fn selected(&self, line: &PDFSelection, page: &PDFPage) -> Option<(String, Rect)> {
        let mut texts = Vec::new();
        let mut bounds: Option<Rect> = None;
        for i in 0..unsafe { line.numberOfTextRangesOnPage(page) } {
            let range = unsafe { line.rangeAtIndex_onPage(i, page) };
            let range = range.location..(range.location + range.length).min(self.units.len());
            let range = trimmed(&self.parts, range);
            if range.is_empty() {
                continue;
            }
            let place = unsafe { page.selectionForRange(NSRange::new(range.start, range.len())) }?;
            let part = rect_of(unsafe { place.boundsForPage(page) });
            bounds = Some(bounds.map_or(part, |whole| union(whole, part)));
            texts.push(String::from_utf16_lossy(&self.units[range]));
        }
        let text = texts.join(" ").trim().to_string();
        (!text.is_empty()).then_some((text, bounds?))
    }

    /// The words on the band of the page at the vertical extent of `bounds`
    /// (less the inset), across the whole width of the page, whose centre lies
    /// outside the width of `bounds`: each with the centre of its bounds.
    fn margin(&self, bounds: Rect) -> Vec<Run> {
        let inset = BAND_INSET * bounds.height;
        let (low, high) = (bounds.y + inset, bounds.y + bounds.height - inset);
        self.parts
            .iter()
            .filter_map(|part| {
                let r = part.bounds;
                let (x, y) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
                let beside = x < bounds.x || x > bounds.x + bounds.width;
                ((low..=high).contains(&y) && beside).then(|| Run {
                    text: String::from_utf16_lossy(&self.units[part.range.clone()]),
                    x,
                })
            })
            .collect()
    }
}

const EMPTY: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 0.0,
    height: 0.0,
};

fn rect_of(r: NSRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

fn union(a: Rect, b: Rect) -> Rect {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    Rect {
        x,
        y,
        width: (a.x + a.width).max(b.x + b.width) - x,
        height: (a.y + a.height).max(b.y + b.height) - y,
    }
}

/// The ranges of the words in a string (UTF-16 units), split at white space.
fn ranges(units: &[u16]) -> Vec<Range<usize>> {
    let blank = |unit: u16| char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace);
    let mut ranges = Vec::new();
    let mut start = None;
    for (i, &unit) in units.iter().enumerate() {
        match (blank(unit), start) {
            (false, None) => start = Some(i),
            (true, Some(from)) => {
                ranges.push(from..i);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(from) = start {
        ranges.push(from..units.len());
    }
    ranges
}

fn is_digit(unit: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&unit)
}

/// Whether a word has digits and other characters: a number may be glued to it.
fn glued(word: &[u16]) -> bool {
    word.iter().any(|&u| is_digit(u)) && word.iter().any(|&u| !is_digit(u))
}

/// A word cut into the pieces it has on the page, from the bounds of its
/// characters: where the next lies far from the one before (further than `GAP`
/// heights to the right, or wholly to the left), another piece begins. A piece
/// of digits beside others is a number apart.
fn pieces(word: &[u16], chars: &[Rect]) -> Vec<(Range<usize>, bool)> {
    let mut pieces = Vec::new();
    let mut start = 0;
    let mut last: Option<Rect> = None;
    for (i, r) in chars.iter().enumerate() {
        // no bounds: the character goes with the one before
        if r.width <= 0.0 {
            continue;
        }
        if let Some(p) = last
            && (r.x > p.x + p.width + GAP * p.height.max(r.height) || r.x + r.width <= p.x)
        {
            pieces.push(start..i);
            start = i;
        }
        last = Some(*r);
    }
    pieces.push(start..word.len());
    let apart = pieces.len() > 1
        && pieces
            .iter()
            .any(|p| !word[p.clone()].iter().all(|&u| is_digit(u)));
    pieces
        .into_iter()
        .map(|p| {
            let number = apart && word[p.clone()].iter().all(|&u| is_digit(u));
            (p, number)
        })
        .collect()
}

/// A range of the string cut down to the parts in it that are not apart (all
/// of them, if all are), from the first to the last: no white space or glued
/// number at its ends.
fn trimmed(parts: &[Part], range: Range<usize>) -> Range<usize> {
    let within: Vec<&Part> = parts
        .iter()
        .filter(|p| p.range.start < range.end && p.range.end > range.start)
        .collect();
    let text: Vec<&Part> = within.iter().copied().filter(|p| !p.apart).collect();
    let kept = if text.is_empty() { &within } else { &text };
    match (kept.first(), kept.last()) {
        (Some(first), Some(last)) => {
            first.range.start.max(range.start)..last.range.end.min(range.end)
        }
        _ => range.start..range.start,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// The characters of `text`, 5 points wide from `x`, 10 high.
    fn chars(text: &str, x: f64) -> Vec<Rect> {
        (0..text.encode_utf16().count())
            .map(|i| rect(x + 5.0 * i as f64, 100.0, 5.0, 10.0))
            .collect()
    }

    #[test]
    fn ranges_split_at_white_space() {
        assert_eq!(ranges(&units("12\n Der  Hund ")), [0..2, 4..7, 9..13]);
        assert_eq!(ranges(&units("a\n\n b")), [0..1, 4..5]);
        assert!(ranges(&units(" \n")).is_empty());
        // counted in UTF-16 units: a letter outside the plane is two
        assert_eq!(ranges(&units("𝔄 b")), [0..2, 3..4]);
    }

    #[test]
    fn a_number_glued_to_a_word_is_apart_where_it_lies_apart() {
        // "gravida" at 100, the line number "10" in the margin at 10
        let word = units("gravida10");
        let mut at = chars("gravida", 100.0);
        at.extend(chars("10", 10.0));
        assert_eq!(pieces(&word, &at), [(0..7, false), (7..9, true)]);
        // "10" at the start of the word, to the left of the text
        let word = units("10gravida");
        let mut at = chars("10", 10.0);
        at.extend(chars("gravida", 100.0));
        assert_eq!(pieces(&word, &at), [(0..2, true), (2..9, false)]);
        // "In47", the two letters and the two digits at a distance
        let mut at = chars("In", 238.0);
        at.extend(chars("47", 500.0));
        assert_eq!(pieces(&units("In47"), &at), [(0..2, false), (2..4, true)]);
    }

    #[test]
    fn a_word_in_one_piece_stays_whole() {
        let at = chars("H2O", 100.0);
        assert_eq!(pieces(&units("H2O"), &at), [(0..3, false)]);
        // a character without bounds goes with the one before
        let mut at = chars("a1", 100.0);
        at[1] = EMPTY;
        assert_eq!(pieces(&units("a1"), &at), [(0..2, false)]);
        // two pieces of digits only are not numbers apart
        let mut at = chars("12", 10.0);
        at.extend(chars("34", 100.0));
        assert_eq!(pieces(&units("1234"), &at), [(0..2, false), (2..4, false)]);
        assert!(glued(&units("gravida10")) && !glued(&units("1234")) && !glued(&units("abc")));
    }

    fn part(range: Range<usize>, bounds: Rect, apart: bool) -> Part {
        Part {
            range,
            bounds,
            apart,
        }
    }

    #[test]
    fn a_range_is_cut_down_to_the_text_without_the_numbers_apart() {
        // "tortor gravida10 and" (units 0..20): 'gravida' 7..14, '10' apart 14..16
        let parts = vec![
            part(0..6, EMPTY, false),
            part(7..14, EMPTY, false),
            part(14..16, EMPTY, true),
            part(17..20, EMPTY, false),
        ];
        assert_eq!(trimmed(&parts, 0..16), 0..14);
        assert_eq!(trimmed(&parts, 3..17), 3..14);
        assert_eq!(trimmed(&parts, 0..20), 0..20);
        // a number apart at the start
        let parts = vec![part(0..2, EMPTY, true), part(2..9, EMPTY, false)];
        assert_eq!(trimmed(&parts, 0..9), 2..9);
        // only a number apart, or no part at all
        assert_eq!(trimmed(&parts[..1], 0..3), 0..2);
        assert!(trimmed(&parts, 20..30).is_empty());
    }

    #[test]
    fn the_margin_is_the_words_of_the_band_outside_the_part_selected() {
        // a line of 10 points at y = 100, selected from x = 100 to 300
        let selected = rect(100.0, 100.0, 200.0, 10.0);
        let text = Text {
            units: units("12 in 13 11 14"),
            parts: vec![
                part(0..2, rect(40.0, 101.0, 10.0, 8.0), false),
                part(3..5, rect(140.0, 100.0, 20.0, 10.0), false),
                part(6..8, rect(340.0, 101.0, 10.0, 8.0), false),
                // the line above and the line below
                part(9..11, rect(40.0, 113.0, 10.0, 8.0), false),
                part(12..14, rect(40.0, 89.0, 10.0, 8.0), false),
            ],
        };
        let margin = text.margin(selected);
        let seen: Vec<(&str, f64)> = margin.iter().map(|r| (r.text.as_str(), r.x)).collect();
        assert_eq!(seen, [("12", 45.0), ("13", 345.0)]);
    }

    #[test]
    fn union_of_rects() {
        let whole = union(rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 5.0, 5.0, 10.0));
        assert_eq!(whole, rect(0.0, 0.0, 25.0, 15.0));
    }

    /// The path of a PDF in `PDFKIT_TEST_PDF` and a text in `PDFKIT_TEST_FIND`:
    /// every line of the selection of the first place of the text (as far as
    /// the lines it lies on with `PDFKIT_TEST_LINE` set), with the words beside
    /// each line. Run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lines_of_a_text_found_in_a_pdf() {
        let path = std::env::var("PDFKIT_TEST_PDF").expect("PDFKIT_TEST_PDF: a PDF");
        let find = std::env::var("PDFKIT_TEST_FIND").expect("PDFKIT_TEST_FIND: a text");
        unsafe {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path));
            let document = PDFDocument::initWithURL(PDFDocument::alloc(), &url).expect("a PDF");
            let found = document
                .findString_fromSelection_withOptions(
                    &NSString::from_str(&find),
                    None,
                    NSStringCompareOptions::empty(),
                )
                .expect("the text in the PDF");
            if std::env::var("PDFKIT_TEST_LINE").is_ok() {
                found.extendSelectionForLineBoundaries();
            }
            let lines = lines(&found, &document);
            assert!(!lines.is_empty());
            for line in lines {
                println!("{line:#?}");
            }
        }
    }
}
