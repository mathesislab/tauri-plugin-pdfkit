# tauri-plugin-pdfkit

A PDF in a native view of PDFKit, as Preview shows it, over an area of the
page of a [Tauri 2](https://tauri.app) web view (macOS). The page keeps the
area in place and drives the view: find, zoom, the outline, back and forward, the
pages; the view tells the page the page in view and its history. One view per web view: a window may hold several
(Tauri's multiwebview).

Elsewhere than on macOS the commands fail, and the page shows the PDF
itself (with pdf.js, say).

Made for [Papers Desktop](https://github.com/mathesislab/papers-desktop),
where it began (`src-tauri/src/pdf_view.rs`), and
[papers-review](https://github.com/mathesislab/papers-review).

## Install

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-pdfkit = { git = "https://github.com/mathesislab/tauri-plugin-pdfkit", tag = "v0.7.1" }
```

```bash
npm install github:mathesislab/tauri-plugin-pdfkit#v0.7.1
```

The crate and the bindings go together: the same tag for both.

```rust
tauri::Builder::default()
    // the points of a CSS pixel, when the app zooms its pages; 1 without `zoom`
    .plugin(tauri_plugin_pdfkit::Builder::new().zoom(|app| text_zoom(app)).build())
```

and `"pdfkit:default"` in the permissions of the capability of the web view.

## Use

```ts
import { cover, frameOf, onPage, open, setFrame, close } from "tauri-plugin-pdfkit-api";

const { count, outline } = await open("paper-1", "/path/to/paper.pdf", 1, frameOf(area));
const unlisten = await onPage((page) => (current = page));
new ResizeObserver(() => setFrame(frameOf(area))).observe(area);
// a modal of the page: the view lies above all of the page, so it hides
await cover("paper-1", true);
await close("paper-1");
```

| Command | Does |
|---|---|
| `open(id, path, page, frame, covered, clicks)` | shows `path` at `page` over `frame`, in place of the PDF the web view shows; the page count and the outline. With `clicks` the page takes ⌘-clicks and right-clicks (`onClick`); without, PDFKit keeps them, its own menu with them |
| `setFrame(frame)` | moves the view with its area |
| `cover(id, covered)` | hides the view while a modal of the page is open, and gives the keys to the page; shown again, the view has the keys back if it had them (a text beside it keeps them otherwise). On a view shown, `cover(id, false)` gives it the keys |
| `close(id)` | takes the view away |
| `find(text, again, backwards)` | marks all the places, selects the first, the next or the one before; an empty text clears |
| `zoom(step)` | in (1), out (-1), the width of the area (0) |
| `go(target)` | goes to an entry of the outline (each entry says the page it goes to) |
| `back()`, `forward()` | back to the page the last link or jump left, and forward again |
| `pageTo(page)` | goes to `page` (from 1) |
| `turn(to)` | the `first`, `previous`, `next` or `last` page |
| `selection()` | the text selected, line by line: the page and label, the part selected in page points, and the words on the same band of the page outside it (the line numbers of a manuscript among them), and the numbers standing alone on its pages with their place (a manuscript numbered every fifth line); null if nothing is selected |
| `place(page, x, y)` | the place at a point of a page, in page points from the top left of its crop box (as SyncTeX has it): the page's text as PDFKit reads it, where the word at the point begins in it and the line there (UTF-16 units, as JavaScript counts) |
| `mark(page, from, to)` | marks that part of the page's text, as a highlighter would, and scrolls to it; an empty range takes the mark away, as does the next click |
| `onPage(handler)` | the page in view, as it changes (event `pdfkit:page`, to the web view) |
| `onHistory(handler)` | `{ back, forward }`, whether there is a page to go back to or forward, as it changes (event `pdfkit:history`) |
| `onClick(handler)` | with `clicks`: a ⌘-click, or a right-click (`menu`), on the PDF: where in the page (`client`, CSS pixels), the place clicked and the text selected (event `pdfkit:click`). PDFKit shows no menu of its own: the page shows its own |

The views of a window go when the window is destroyed, and the view of a
web view when its page loads again.

## Versions

- 0.7.1: a view shown again after a modal has the keys back only if it had
  them: the page's own text beside it keeps them.
- 0.7.0: the clicks are the page's only when it opens the PDF with `clicks`;
  otherwise PDFKit keeps them and its menu, as before 0.6.0.
- 0.6.0: the page of each entry of the outline; `place` and `mark`, for a
  place in the text of a page (a SyncTeX map's) found and marked; ⌘-clicks
  and right-clicks told to the page (`onClick`), which shows its own menu.
- 0.5.0: `selection` gives the numbers standing alone on the pages of the
  selection too (`numbers`), for the line numbers of the lines that have none
  printed beside them.
- 0.4.0: `back`, `forward`, `pageTo`, `turn`, and the history told to the
  page (`onHistory`): a link followed in the PDF can be gone back from.
- 0.3.0: `selection`, the text selected with the geometry to read the line
  numbers printed in the margins.
- 0.2.1: the view of a web view goes when its page loads again (a reload):
  nothing keeps it in place any more.
- 0.2.0: the commands take the web view that calls them, not its window:
  a window may hold several web views. The page event goes to the web view.
- 0.1.0: from papers-desktop.

## Develop

```bash
cargo clippy --all-targets -- -D warnings
npm install && npm run build   # guest-js → dist-js, committed for git installs
```

## License

MIT.
