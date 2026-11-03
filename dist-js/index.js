// A PDF in a native view of PDFKit (macOS), over an area of the page: the
// page keeps the area in place and drives the view through these commands.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
/** The frame of an element of the page. */
export function frameOf(el) {
    const box = el.getBoundingClientRect();
    return { x: box.left, y: box.top, width: box.width, height: box.height, viewport: window.innerHeight };
}
/** Open `path` at `page` (from 1) over `frame`, in place of the PDF the
 *  web view shows; hidden while `covered`. `id` names it for `cover` and `close`. */
export function open(id, path, page, frame, covered = false) {
    return invoke("plugin:pdfkit|open", { id, path, page, frame, covered });
}
/** The area moved or changed size. */
export function setFrame(frame) {
    return invoke("plugin:pdfkit|frame", { frame });
}
/** A modal of the page opened (`covered`) or closed: the view lies above all
 *  of the page, so it hides while a modal shows. */
export function cover(id, covered) {
    return invoke("plugin:pdfkit|cover", { id, covered });
}
/** The PDF `id` is not in front any more: its view goes. */
export function close(id) {
    return invoke("plugin:pdfkit|close", { id });
}
/** Find `text`: the first place, or `again` the next (the one before,
 *  `backwards`). Whether found; an empty text clears. */
export function find(text, again = false, backwards = false) {
    return invoke("plugin:pdfkit|find", { text, again, backwards });
}
/** In (1) or out (-1), or the width of the area (0). */
export function zoom(step) {
    return invoke("plugin:pdfkit|zoom", { step });
}
/** Go to an entry of the outline. */
export function go(target) {
    return invoke("plugin:pdfkit|go", { target });
}
/** Back to the page the last link or jump left (see `onHistory`). */
export function back() {
    return invoke("plugin:pdfkit|back");
}
/** Forward again, after going back. */
export function forward() {
    return invoke("plugin:pdfkit|forward");
}
/** Go to `page` (from 1; the last, if beyond it). */
export function pageTo(page) {
    return invoke("plugin:pdfkit|page_to", { page });
}
/** Turn to the first, previous, next or last page. */
export function turn(to) {
    return invoke("plugin:pdfkit|turn", { to });
}
/** The text selected in the PDF, line by line, with the words beside each line
 *  (the line numbers of a manuscript among them); null if nothing is selected. */
export function selection() {
    return invoke("plugin:pdfkit|selection");
}
/** The page in view (from 1), as it changes, in this web view. */
export function onPage(handler) {
    return getCurrentWebview().listen("pdfkit:page", (e) => handler(e.payload));
}
/** The history, when it changes, in this web view. */
export function onHistory(handler) {
    return getCurrentWebview().listen("pdfkit:history", (e) => handler(e.payload));
}
