// A PDF in a native view of PDFKit (macOS), over an area of the page: the
// page keeps the area in place and drives the view through these commands.

import { invoke } from "@tauri-apps/api/core";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";

/** The place of the area in the page, in CSS pixels, and the height of the
 *  page (mirrors Frame). */
export type Frame = { x: number; y: number; width: number; height: number; viewport: number };

/** An entry of the outline; `target`, the indices that lead to it from the
 *  root (mirrors Entry). */
export type Entry = { title: string; target: number[]; items: Entry[] };

/** What the page learns of a PDF opened (mirrors Opened). */
export type Opened = { count: number; outline: Entry[] };

/** The frame of an element of the page. */
export function frameOf(el: HTMLElement): Frame {
  const box = el.getBoundingClientRect();
  return { x: box.left, y: box.top, width: box.width, height: box.height, viewport: window.innerHeight };
}

/** Open `path` at `page` (from 1) over `frame`, in place of the PDF the
 *  web view shows; hidden while `covered`. `id` names it for `cover` and `close`. */
export function open(id: string, path: string, page: number, frame: Frame, covered = false): Promise<Opened> {
  return invoke<Opened>("plugin:pdfkit|open", { id, path, page, frame, covered });
}

/** The area moved or changed size. */
export function setFrame(frame: Frame): Promise<void> {
  return invoke("plugin:pdfkit|frame", { frame });
}

/** A modal of the page opened (`covered`) or closed: the view lies above all
 *  of the page, so it hides while a modal shows. */
export function cover(id: string, covered: boolean): Promise<void> {
  return invoke("plugin:pdfkit|cover", { id, covered });
}

/** The PDF `id` is not in front any more: its view goes. */
export function close(id: string): Promise<void> {
  return invoke("plugin:pdfkit|close", { id });
}

/** Find `text`: the first place, or `again` the next (the one before,
 *  `backwards`). Whether found; an empty text clears. */
export function find(text: string, again = false, backwards = false): Promise<boolean> {
  return invoke<boolean>("plugin:pdfkit|find", { text, again, backwards });
}

/** In (1) or out (-1), or the width of the area (0). */
export function zoom(step: number): Promise<void> {
  return invoke("plugin:pdfkit|zoom", { step });
}

/** Go to an entry of the outline. */
export function go(target: number[]): Promise<void> {
  return invoke("plugin:pdfkit|go", { target });
}

/** The page in view (from 1), as it changes, in this web view. */
export function onPage(handler: (page: number) => void): Promise<UnlistenFn> {
  return getCurrentWebview().listen<number>("pdfkit:page", (e) => handler(e.payload));
}
