//! Elsewhere than on macOS there is no PDFKit: the commands fail, and the
//! page shows the PDF itself.

use std::path::PathBuf;

use tauri::{Runtime, Webview};

use crate::{Frame, Opened, Selection};

const UNSUPPORTED: &str = "PDFKit: macOS only";

pub fn open<R: Runtime>(
    _: Webview<R>,
    _: String,
    _: PathBuf,
    _: usize,
    _: Frame,
    _: bool,
    _: f64,
) -> Result<Opened, String> {
    Err(UNSUPPORTED.into())
}

pub fn set_frame<R: Runtime>(_: Webview<R>, _: Frame, _: f64) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn cover<R: Runtime>(_: Webview<R>, _: String, _: bool) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn close<R: Runtime>(_: Webview<R>, _: String) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn destroyed(_: &str) {}

pub fn unload<R: Runtime>(_: &Webview<R>) {}

pub fn find<R: Runtime>(_: Webview<R>, _: String, _: bool, _: bool) -> Result<bool, String> {
    Err(UNSUPPORTED.into())
}

pub fn zoom<R: Runtime>(_: Webview<R>, _: i32) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn go<R: Runtime>(_: Webview<R>, _: Vec<usize>) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn selection<R: Runtime>(_: Webview<R>) -> Result<Option<Selection>, String> {
    Ok(None)
}
