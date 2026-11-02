# tauri-plugin-pdfkit

A PDF in a native view of PDFKit, as Preview shows it, over an area of the
page of a [Tauri 2](https://tauri.app) window (macOS). The page keeps the
area in place and drives the view: find, zoom, the outline; the view tells
the page the page in view. One view per window.

Elsewhere than on macOS the commands fail, and the page shows the PDF
itself (with pdf.js, say).

Made for [Papers Desktop](https://github.com/mathesislab/papers-desktop),
where it began (`src-tauri/src/pdf_view.rs`), and
[papers-review](https://github.com/mathesislab/papers-review).

## Install

```toml
# src-tauri/Cargo.toml
[dependencies]
tauri-plugin-pdfkit = { git = "https://github.com/mathesislab/tauri-plugin-pdfkit" }
```

```bash
npm install github:mathesislab/tauri-plugin-pdfkit
```

```rust
tauri::Builder::default()
    // the points of a CSS pixel, when the app zooms its pages; 1 without `zoom`
    .plugin(tauri_plugin_pdfkit::Builder::new().zoom(|app| text_zoom(app)).build())
```

and `"pdfkit:default"` in the permissions of the window's capability.

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
| `open(id, path, page, frame, covered)` | shows `path` at `page` over `frame`, in place of the PDF the window shows; the page count and the outline |
| `setFrame(frame)` | moves the view with its area |
| `cover(id, covered)` | hides the view while a modal of the page is open, and gives the keys to the page |
| `close(id)` | takes the view away |
| `find(text, again, backwards)` | marks all the places, selects the first, the next or the one before; an empty text clears |
| `zoom(step)` | in (1), out (-1), the width of the area (0) |
| `go(target)` | goes to an entry of the outline |
| `onPage(handler)` | the page in view, as it changes (event `pdfkit:page`) |

The view of a window goes when the window is destroyed.

## Develop

```bash
cargo clippy --all-targets -- -D warnings
npm install && npm run build   # guest-js → dist-js, committed for git installs
```

## License

MIT.
