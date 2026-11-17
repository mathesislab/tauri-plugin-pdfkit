import type { UnlistenFn } from "@tauri-apps/api/event";
/** The place of the area in the page, in CSS pixels, and the height of the
 *  page (mirrors Frame). */
export type Frame = {
    x: number;
    y: number;
    width: number;
    height: number;
    viewport: number;
};
/** An entry of the outline; `target`, the indices that lead to it from the
 *  root; `page`, the page it goes to (from 1; null if none; mirrors Entry). */
export type Entry = {
    title: string;
    target: number[];
    page: number | null;
    items: Entry[];
};
/** What the page learns of a PDF opened (mirrors Opened). */
export type Opened = {
    count: number;
    outline: Entry[];
};
/** A rectangle of a page, in page points, the origin at the bottom left as
 *  PDFKit has it (mirrors Rect). */
export type Rect = {
    x: number;
    y: number;
    width: number;
    height: number;
};
/** A word of the page and the centre of its bounds (x, page points; mirrors
 *  Run). */
export type Run = {
    text: string;
    x: number;
};
/** A line of the selection: its page (from 1) and printed label ("" if none),
 *  the part selected (page points, origin bottom left as PDFKit has it), and
 *  the words on the same band of the page outside it, the line numbers printed
 *  beside the text among them (mirrors Line). */
export type Line = {
    page: number;
    label: string;
    text: string;
    bounds: Rect;
    margin: Run[];
};
/** A whole number standing alone on a page, and the centre of its bounds
 *  (page points): a line number printed in the margin, a page number, or a
 *  number in the text (mirrors Mark). */
export type Mark = {
    page: number;
    n: number;
    x: number;
    y: number;
};
/** The text selected in the PDF, line by line, and the numbers standing alone
 *  on its pages, top to bottom: with them, the line number of a line that has
 *  none printed beside it (mirrors Selection). */
export type Selection = {
    text: string;
    lines: Line[];
    numbers: Mark[];
};
/** Whether there is a page to go back to (the one a link or a jump left), or
 *  forward (after going back; mirrors History). */
export type History = {
    back: boolean;
    forward: boolean;
};
/** A place on a page (from 1): its point, in page points from the top left of
 *  the page's crop box, as SyncTeX and pdf.js have it; the page's text, as
 *  PDFKit reads it; where the word at the point begins in it (the line's start
 *  off a word; null off the text), and the line there, from and to (mirrors
 *  Place). */
export type Place = {
    page: number;
    x: number;
    y: number;
    text: string;
    index: number | null;
    line: [number, number] | null;
};
/** A point of the page, in CSS pixels from the top left of its viewport, as a
 *  mouse event's clientX and clientY (mirrors Point). */
export type Point = {
    x: number;
    y: number;
};
/** A ⌘-click on the PDF, or a right-click (`menu`: PDFKit shows no menu of its
 *  own, for the page to show its own at `client`): the place clicked (null off
 *  the pages) and the text selected ("" if none; mirrors Click). */
export type Click = {
    menu: boolean;
    client: Point;
    place: Place | null;
    selected: string;
};
/** Where to turn from the page in view (mirrors Turn). */
export type Turn = "first" | "previous" | "next" | "last";
/** The frame of an element of the page. */
export declare function frameOf(el: HTMLElement): Frame;
/** Open `path` at `page` (from 1) over `frame`, in place of the PDF the
 *  web view shows; hidden while `covered`. `id` names it for `cover` and `close`.
 *  With `clicks`, the page takes a ⌘-click and a right-click (`onClick`, its own
 *  menu) from PDFKit; without, PDFKit has them (its own menu). */
export declare function open(id: string, path: string, page: number, frame: Frame, covered?: boolean, clicks?: boolean): Promise<Opened>;
/** The area moved or changed size. */
export declare function setFrame(frame: Frame): Promise<void>;
/** A modal of the page opened (`covered`) or closed: the view lies above all
 *  of the page, so it hides while a modal shows, and has the keys back after
 *  only if it had them. On a view shown, `cover(id, false)` gives it the keys. */
export declare function cover(id: string, covered: boolean): Promise<void>;
/** The PDF `id` is not in front any more: its view goes. */
export declare function close(id: string): Promise<void>;
/** Find `text`: the first place, or `again` the next (the one before,
 *  `backwards`). Whether found; an empty text clears. */
export declare function find(text: string, again?: boolean, backwards?: boolean): Promise<boolean>;
/** In (1) or out (-1), or the width of the area (0). */
export declare function zoom(step: number): Promise<void>;
/** Go to an entry of the outline. */
export declare function go(target: number[]): Promise<void>;
/** Back to the page the last link or jump left (see `onHistory`). */
export declare function back(): Promise<void>;
/** Forward again, after going back. */
export declare function forward(): Promise<void>;
/** Go to `page` (from 1; the last, if beyond it). */
export declare function pageTo(page: number): Promise<void>;
/** Turn to the first, previous, next or last page. */
export declare function turn(to: Turn): Promise<void>;
/** The text selected in the PDF, line by line, with the words beside each line
 *  (the line numbers of a manuscript among them); null if nothing is selected. */
export declare function selection(): Promise<Selection | null>;
/** The place at a point of `page` (from 1), in page points from the top left
 *  of its crop box (as SyncTeX has it); null if there is no such page. */
export declare function place(page: number, x: number, y: number): Promise<Place | null>;
/** The text of `page` from `from` to `to` (of its text, as `Place` has it)
 *  marked and scrolled to; an empty range takes the mark away. Whether marked. */
export declare function mark(page: number, from: number, to: number): Promise<boolean>;
/** The page in view (from 1), as it changes, in this web view. */
export declare function onPage(handler: (page: number) => void): Promise<UnlistenFn>;
/** The history, when it changes, in this web view. */
export declare function onHistory(handler: (history: History) => void): Promise<UnlistenFn>;
/** A ⌘-click or a right-click on the PDF, in this web view, opened with
 *  `clicks`. */
export declare function onClick(handler: (click: Click) => void): Promise<UnlistenFn>;
