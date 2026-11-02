//! Elsewhere than on macOS there is no PDFKit: the commands fail, and the
//! page shows the PDF itself.

use std::path::PathBuf;

use tauri::{Runtime, WebviewWindow};

use crate::{Frame, Opened};

const UNSUPPORTED: &str = "PDFKit: macOS only";

pub fn open<R: Runtime>(
    _: WebviewWindow<R>,
    _: String,
    _: PathBuf,
    _: usize,
    _: Frame,
    _: bool,
    _: f64,
) -> Result<Opened, String> {
    Err(UNSUPPORTED.into())
}

pub fn set_frame<R: Runtime>(_: WebviewWindow<R>, _: Frame, _: f64) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn cover<R: Runtime>(_: WebviewWindow<R>, _: String, _: bool) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn close<R: Runtime>(_: WebviewWindow<R>, _: String) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn destroyed(_: &str) {}

pub fn find<R: Runtime>(_: WebviewWindow<R>, _: String, _: bool, _: bool) -> Result<bool, String> {
    Err(UNSUPPORTED.into())
}

pub fn zoom<R: Runtime>(_: WebviewWindow<R>, _: i32) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}

pub fn go<R: Runtime>(_: WebviewWindow<R>, _: Vec<usize>) -> Result<(), String> {
    Err(UNSUPPORTED.into())
}
