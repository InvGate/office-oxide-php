//! # office_oxide_php
//!
//! A PHP extension that reads Microsoft Office documents — DOCX, DOC, XLSX, XLS,
//! PPTX, and PPT — by wrapping the pure-Rust [`office_oxide`] crate. No C/C++
//! dependencies and no external binaries.
//!
//! The extension exposes a single class, `OfficeOxide\Document`, plus an
//! `OfficeOxide\OfficeException` thrown on read errors.
//!
//! ```php
//! use OfficeOxide\Document;
//!
//! $doc = Document::open('report.docx');
//! echo $doc->getText();
//! echo $doc->toHtml();
//! $ir = $doc->getIr();          // nested PHP array
//! ```

// On Windows, ext-php-rs relies on the `vectorcall` ABI, which is nightly-only.
#![cfg_attr(windows, feature(abi_vectorcall))]

mod ir;

use ext_php_rs::binary::Binary;
use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use ext_php_rs::zend::ce;

use office_oxide::{Document as OxideDocument, DocumentFormat};

/// Exception thrown by `OfficeOxide\Document` when a document cannot be read.
///
/// Extends PHP's built-in `\Exception`, so it can be caught with a plain
/// `catch (\Exception $e)` or specifically as `OfficeOxide\OfficeException`.
#[php_class]
#[php(name = "OfficeOxide\\OfficeException")]
#[php(extends(ce = ce::exception, stub = "\\Exception"))]
#[derive(Default)]
pub struct OfficeException;

/// Build an `OfficeException` carrying the upstream error message.
fn office_exception(message: impl std::fmt::Display) -> PhpException {
    PhpException::from_class::<OfficeException>(message.to_string())
}

/// A read-only handle to an Office document.
///
/// Exported to PHP as `OfficeOxide\Document`. Construct one with the static
/// `Document::open($path)` or `Document::fromString($bytes, $format)` factories.
#[php_class]
#[php(name = "OfficeOxide\\Document")]
pub struct Document {
    inner: OxideDocument,
}

#[php_impl]
impl Document {
    /// Open a document from a filesystem path. The format is detected from the
    /// file extension. Throws `OfficeOxide\OfficeException` on failure.
    pub fn open(path: String) -> PhpResult<Self> {
        let inner = OxideDocument::open(&path).map_err(office_exception)?;
        Ok(Self { inner })
    }

    /// Open a document from an in-memory byte string. `format` is a file
    /// extension such as `"docx"`, `"doc"`, `"xlsx"`, `"xls"`, `"pptx"`, or
    /// `"ppt"`. Throws `OfficeOxide\OfficeException` on an unknown format or a
    /// read error.
    pub fn from_string(data: Binary<u8>, format: String) -> PhpResult<Self> {
        let fmt = DocumentFormat::from_extension(&format)
            .ok_or_else(|| office_exception(format!("unsupported format: {format}")))?;
        let bytes: Vec<u8> = data.to_vec();
        let inner =
            OxideDocument::from_reader(std::io::Cursor::new(bytes), fmt).map_err(office_exception)?;
        Ok(Self { inner })
    }

    /// Extract the document's plain text.
    pub fn get_text(&self) -> String {
        self.inner.plain_text()
    }

    /// Render the document as an HTML fragment.
    pub fn to_html(&self) -> String {
        self.inner.to_html()
    }

    /// Render the document as Markdown.
    pub fn to_markdown(&self) -> String {
        self.inner.to_markdown()
    }

    /// The document's format as a lowercase extension string, e.g. `"docx"`.
    pub fn get_format(&self) -> String {
        self.inner.format().extension().to_string()
    }

    /// Document metadata (title, author, dates, ...) as a PHP array.
    pub fn get_metadata(&self) -> PhpResult<Zval> {
        let ir = self.inner.to_ir();
        let value = serde_json::to_value(&ir.metadata).map_err(office_exception)?;
        Ok(ir::json_to_zval(&value))
    }

    /// The full structured intermediate representation as a nested PHP array
    /// (`metadata` plus an ordered list of `sections`).
    pub fn get_ir(&self) -> PhpResult<Zval> {
        let ir = self.inner.to_ir();
        let value = serde_json::to_value(&ir).map_err(office_exception)?;
        Ok(ir::json_to_zval(&value))
    }

    /// The full structured intermediate representation serialized as a JSON
    /// string — convenient for persisting or forwarding without walking the
    /// array in PHP.
    pub fn to_json(&self) -> PhpResult<String> {
        let ir = self.inner.to_ir();
        serde_json::to_string(&ir).map_err(office_exception)
    }
}

/// Register the extension's classes with PHP.
#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module.class::<OfficeException>().class::<Document>()
}
