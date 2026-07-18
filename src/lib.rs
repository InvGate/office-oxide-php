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

mod ser;

use ext_php_rs::binary::Binary;
use ext_php_rs::convert::IntoZval;
use ext_php_rs::exception::PhpException;
use ext_php_rs::prelude::*;
use ext_php_rs::types::{ZendHashTable, Zval};
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
        // Move the owned buffer out of `Binary` rather than cloning it.
        let bytes: Vec<u8> = data.into();
        let inner = OxideDocument::from_reader(std::io::Cursor::new(bytes), fmt)
            .map_err(office_exception)?;
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
        let mut ctx = ser::Context::for_tree(false);
        ser::to_zval(&ir.metadata, &mut ctx).map_err(office_exception)
    }

    /// The full structured intermediate representation as a nested PHP array
    /// (`metadata` plus an ordered list of `sections`).
    ///
    /// Each image element carries an ordinal `image_id` (document order). Image
    /// bytes are **omitted by default** to avoid a large memory blow-up — fetch
    /// them with `getImages()`, correlated by `image_id`. Pass
    /// `includeImageData: true` to inline the bytes as PHP binary strings.
    #[php(defaults(include_image_data = false))]
    pub fn get_ir(&self, include_image_data: bool) -> PhpResult<Zval> {
        let ir = self.inner.to_ir();
        let mut ctx = ser::Context::for_tree(include_image_data);
        ser::to_zval(&ir, &mut ctx).map_err(office_exception)
    }

    /// Every embedded image, as a list of associative arrays.
    ///
    /// Without `outputDir`, each entry is
    /// `['image_id' => int, 'format' => ?string, 'data' => ?string]`, where
    /// `data` is a raw PHP **binary string** (or `null` when the source carried
    /// no extractable bytes).
    ///
    /// With `outputDir`, each image is written to
    /// `{outputDir}/image_{image_id}.{ext}` and the entry becomes
    /// `['image_id' => int, 'format' => ?string, 'path' => ?string]` instead —
    /// so the caller never holds all image bytes in PHP memory at once. The
    /// directory is created if missing; `path` is `null` for images with no
    /// bytes.
    ///
    /// `image_id` matches the value on the corresponding element in `getIr()` —
    /// both walk the same document-order tree, so the ids line up without any
    /// shared state.
    pub fn get_images(&self, output_dir: Option<String>) -> PhpResult<Zval> {
        // Directory mode returns a `path` key; default mode returns `data`.
        let is_dir_mode = output_dir.is_some();
        if let Some(dir) = &output_dir {
            std::fs::create_dir_all(dir).map_err(office_exception)?;
        }

        let ir = self.inner.to_ir();
        // Each image is materialised (as a binary string, or written to disk)
        // as it is walked, so `ctx.images` holds finished PHP values, not a pile
        // of raw byte vectors. The structural tree is discarded.
        let mut ctx = ser::Context::for_images(output_dir);
        let _ = ser::to_zval(&ir, &mut ctx).map_err(office_exception)?;
        // The IR clone (and its image-byte copies) is no longer needed; free it
        // before building the result array.
        drop(ir);

        let mut list = ZendHashTable::new();
        for img in ctx.images {
            let mut entry = ZendHashTable::new();

            let mut id_z = Zval::new();
            id_z.set_long(img.id);
            let _ = entry.insert("image_id", id_z);

            let fmt_z = match &img.format {
                Some(s) => ser::string_zval(s),
                None => ser::null_zval(),
            };
            let _ = entry.insert("format", fmt_z);

            // `value` is the pre-built binary string (default) or path string
            // (directory mode); null when the image had no bytes.
            let value = img.value.unwrap_or_else(ser::null_zval);
            let _ = entry.insert(if is_dir_mode { "path" } else { "data" }, value);

            let entry_z = entry.into_zval(false).map_err(office_exception)?;
            let _ = list.push(entry_z);
        }
        list.into_zval(false).map_err(office_exception)
    }

    /// The full structured intermediate representation serialized as a JSON
    /// string — convenient for persisting or forwarding without walking the
    /// array in PHP.
    ///
    /// Mirrors `getIr()`: image elements carry `image_id`, and image bytes are
    /// omitted by default. Because JSON cannot hold raw bytes, passing
    /// `includeImageData: true` encodes them as **base64** strings (not the
    /// binary strings the array methods return).
    #[php(defaults(include_image_data = false))]
    pub fn to_json(&self, include_image_data: bool) -> PhpResult<String> {
        let ir = self.inner.to_ir();
        let mut value = serde_json::to_value(&ir).map_err(office_exception)?;
        let mut counter: i64 = 0;
        transform_images_json(&mut value, include_image_data, &mut counter);
        serde_json::to_string(&value).map_err(office_exception)
    }
}

/// Walk a `serde_json::Value` IR tree, assigning each image element an ordinal
/// `image_id` (document order) and applying the image-byte policy: omit `data`
/// by default, or base64-encode it when `include_image_data` is set.
fn transform_images_json(
    value: &mut serde_json::Value,
    include_image_data: bool,
    counter: &mut i64,
) {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let is_image = map.get("type").and_then(Value::as_str) == Some("image");
            if is_image {
                let id = *counter;
                *counter += 1;
                map.insert("image_id".to_string(), Value::from(id));
                if include_image_data {
                    if let Some(Value::Array(arr)) = map.get("data") {
                        let bytes: Vec<u8> = arr
                            .iter()
                            .filter_map(|n| n.as_u64().map(|x| x as u8))
                            .collect();
                        let b64 = base64_encode(&bytes);
                        map.insert("data".to_string(), Value::from(b64));
                    }
                } else {
                    map.remove("data");
                }
            }
            for child in map.values_mut() {
                transform_images_json(child, include_image_data, counter);
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                transform_images_json(item, include_image_data, counter);
            }
        }
        _ => {}
    }
}

/// Standard (RFC 4648) base64 encoding — used for image bytes in `toJson()`.
fn base64_encode(input: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).map_or(0, |&b| b as u32);
        let b2 = chunk.get(2).map_or(0, |&b| b as u32);
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARS[((n >> 18) & 63) as usize] as char);
        out.push(CHARS[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            CHARS[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Register the extension's classes with PHP.
#[php_module]
pub fn get_module(module: ModuleBuilder) -> ModuleBuilder {
    module.class::<OfficeException>().class::<Document>()
}
