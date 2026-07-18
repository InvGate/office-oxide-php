//! Direct `serde` → PHP `Zval` serialization.
//!
//! `office_oxide`'s intermediate representation (`DocumentIR`) is a deep tree of
//! `serde`-serializable structs. The previous approach serialized it to a
//! `serde_json::Value` first and then walked that value into `Zval`s — three
//! full copies of the tree live at once (`DocumentIR`, `serde_json::Value`, and
//! the PHP array).
//!
//! This module removes the middle copy: [`ZvalSerializer`] is a
//! `serde::Serializer` whose `Ok` type is a `Zval`, so a value is walked once,
//! straight into native PHP values.
//!
//! It also solves the *image-byte blow-up*. `office_oxide`'s `Image.data` is an
//! `Option<Vec<u8>>` with a default derive, so `serde` serializes it as a
//! **sequence of `u8`** — every byte would become a full 16-byte `Zval` in a
//! PHP hashtable bucket (a ~16–32× expansion). Instead, when the serializer
//! enters an `Image` struct it:
//!
//! 1. assigns an ordinal `image_id` (document order),
//! 2. by default omits `data` from the tree; when
//!    [`Context::inline_image_data`] is set, emits it as a **binary PHP string**
//!    (never an int array),
//! 3. for `getImages()`, materialises each image's final form *as it is walked*
//!    — a binary-string `Zval`, or a file on disk plus its path — into the
//!    [`Context::images`] side-channel (see [`ImageSink`]). The raw bytes are a
//!    transient buffer dropped at the end of each image, so we never hold every
//!    image's `Vec<u8>` at once.
//!
//! The image node is detected by its serde struct name, `"Image"`, which
//! survives `office_oxide`'s internally-tagged element enums (the `TaggedSerializer`
//! forwards the inner struct's name and prepends the `type` discriminator).

use ext_php_rs::convert::IntoZval;
use ext_php_rs::types::{ZendHashTable, ZendStr, Zval};
use serde::ser::{
    self, Serialize, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant,
    SerializeTuple, SerializeTupleStruct, SerializeTupleVariant, Serializer,
};
use std::fmt::Display;

/// One finished `getImages()` entry, produced as each image is encountered.
///
/// `value` is the already-materialised PHP value — a binary string
/// ([`ImageSink::Binary`]) or a path string ([`ImageSink::Directory`]) — or
/// `None` for an image the source carried no bytes for. Building it eagerly
/// means we never hold every image's raw `Vec<u8>` at once: only one transient
/// buffer exists at a time (during the owning [`StructSer`]).
pub struct CollectedImage {
    pub id: i64,
    pub format: Option<String>,
    pub value: Option<Zval>,
}

/// What to do with each image's bytes as they are encountered.
pub enum ImageSink {
    /// Don't collect (the `getIr()` / `getMetadata()` paths). Bytes are handled
    /// in-tree per [`Context::inline_image_data`] and never captured otherwise.
    None,
    /// `getImages()`: collect each image as a PHP **binary string**.
    Binary,
    /// `getImages($dir)`: write each image to `dir` and collect its **path**,
    /// so the raw bytes never accumulate in memory.
    Directory(String),
}

/// Shared, mutable state threaded through the recursive serialization.
pub struct Context {
    /// Next ordinal id to assign to an image element (document order).
    pub image_counter: i64,
    /// Finished image entries, in the order encountered (only for `getImages`).
    pub images: Vec<CollectedImage>,
    /// Whether image bytes are emitted into the *tree* as binary strings
    /// (`getIr(true)`), or omitted.
    pub inline_image_data: bool,
    /// How `getImages()` wants each image's bytes materialised.
    pub sink: ImageSink,
}

impl Context {
    /// For the tree-producing paths (`getIr`, `getMetadata`): no image
    /// collection; bytes go in-tree only when `inline_image_data` is set.
    pub fn for_tree(inline_image_data: bool) -> Self {
        Self {
            image_counter: 0,
            images: Vec::new(),
            inline_image_data,
            sink: ImageSink::None,
        }
    }

    /// For `getImages()`: collect each image as a binary string, or — when a
    /// directory is given — write it there and collect the path.
    pub fn for_images(output_dir: Option<String>) -> Self {
        Self {
            image_counter: 0,
            images: Vec::new(),
            inline_image_data: false,
            sink: match output_dir {
                Some(dir) => ImageSink::Directory(dir),
                None => ImageSink::Binary,
            },
        }
    }

    /// Whether the raw bytes of an image are needed at all (either to inline
    /// into the tree, or to feed a collection sink). `getIr(false)` needs none.
    fn needs_image_bytes(&self) -> bool {
        self.inline_image_data || !matches!(self.sink, ImageSink::None)
    }
}

/// Serialize any `Serialize` value into a `Zval`, collecting image bytes into
/// `ctx`.
pub fn to_zval<T: Serialize>(value: &T, ctx: &mut Context) -> Result<Zval, SerError> {
    value.serialize(ZvalSerializer { ctx })
}

/// Error type for the `Zval` serializer.
#[derive(Debug)]
pub struct SerError(String);

impl Display for SerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SerError {}

impl ser::Error for SerError {
    fn custom<T: Display>(msg: T) -> Self {
        SerError(msg.to_string())
    }
}

pub fn null_zval() -> Zval {
    let mut z = Zval::new();
    z.set_null();
    z
}

/// Finish a hashtable into a `Zval`, falling back to null on the (practically
/// unreachable) conversion failure — mirrors the original `ir.rs` behaviour.
fn ht_into_zval(ht: ext_php_rs::boxed::ZBox<ZendHashTable>) -> Zval {
    ht.into_zval(false).unwrap_or_else(|_| null_zval())
}

/// Build a binary PHP string `Zval` from raw bytes (binary-safe, unlike
/// `set_string`, which requires UTF-8).
fn binary_string_zval(bytes: &[u8]) -> Zval {
    let mut z = Zval::new();
    z.set_zend_string(ZendStr::new(bytes, false));
    z
}

/// The serializer. Borrows the shared [`Context`]; cheap to reborrow for
/// children.
pub struct ZvalSerializer<'a> {
    ctx: &'a mut Context,
}

impl<'a> Serializer for ZvalSerializer<'a> {
    type Ok = Zval;
    type Error = SerError;

    type SerializeSeq = SeqSer<'a>;
    type SerializeTuple = SeqSer<'a>;
    type SerializeTupleStruct = SeqSer<'a>;
    type SerializeTupleVariant = TupleVariantSer<'a>;
    type SerializeMap = MapSer<'a>;
    type SerializeStruct = StructSer<'a>;
    type SerializeStructVariant = StructVariantSer<'a>;

    fn serialize_bool(self, v: bool) -> Result<Zval, SerError> {
        let mut z = Zval::new();
        z.set_bool(v);
        Ok(z)
    }

    fn serialize_i8(self, v: i8) -> Result<Zval, SerError> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i16(self, v: i16) -> Result<Zval, SerError> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i32(self, v: i32) -> Result<Zval, SerError> {
        self.serialize_i64(v as i64)
    }
    fn serialize_i64(self, v: i64) -> Result<Zval, SerError> {
        let mut z = Zval::new();
        z.set_long(v);
        Ok(z)
    }

    fn serialize_u8(self, v: u8) -> Result<Zval, SerError> {
        self.serialize_i64(v as i64)
    }
    fn serialize_u16(self, v: u16) -> Result<Zval, SerError> {
        self.serialize_i64(v as i64)
    }
    fn serialize_u32(self, v: u32) -> Result<Zval, SerError> {
        self.serialize_i64(v as i64)
    }
    fn serialize_u64(self, v: u64) -> Result<Zval, SerError> {
        // PHP integers are signed 64-bit; values above i64::MAX fall back to
        // double, matching the original number_to_zval logic.
        let mut z = Zval::new();
        if v <= i64::MAX as u64 {
            z.set_long(v as i64);
        } else {
            z.set_double(v as f64);
        }
        Ok(z)
    }

    fn serialize_f32(self, v: f32) -> Result<Zval, SerError> {
        self.serialize_f64(v as f64)
    }
    fn serialize_f64(self, v: f64) -> Result<Zval, SerError> {
        let mut z = Zval::new();
        z.set_double(v);
        Ok(z)
    }

    fn serialize_char(self, v: char) -> Result<Zval, SerError> {
        let mut z = Zval::new();
        let _ = z.set_string(&v.to_string(), false);
        Ok(z)
    }

    fn serialize_str(self, v: &str) -> Result<Zval, SerError> {
        let mut z = Zval::new();
        let _ = z.set_string(v, false);
        Ok(z)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Zval, SerError> {
        // Not reached by office_oxide's IR (Vec<u8> serializes as a seq), but
        // handled correctly for completeness: a binary PHP string.
        Ok(binary_string_zval(v))
    }

    fn serialize_none(self) -> Result<Zval, SerError> {
        Ok(null_zval())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Zval, SerError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Zval, SerError> {
        Ok(null_zval())
    }
    fn serialize_unit_struct(self, _name: &'static str) -> Result<Zval, SerError> {
        Ok(null_zval())
    }
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<Zval, SerError> {
        // Externally-tagged unit variant (e.g. an enum with `rename_all`) → its
        // name as a string, matching serde_json.
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Zval, SerError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Zval, SerError> {
        // Externally-tagged newtype variant → { variant: value }.
        let inner = value.serialize(ZvalSerializer { ctx: self.ctx })?;
        let mut ht = ZendHashTable::new();
        let _ = ht.insert(variant, inner);
        Ok(ht_into_zval(ht))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<SeqSer<'a>, SerError> {
        Ok(SeqSer {
            ctx: self.ctx,
            ht: ZendHashTable::new(),
        })
    }
    fn serialize_tuple(self, _len: usize) -> Result<SeqSer<'a>, SerError> {
        Ok(SeqSer {
            ctx: self.ctx,
            ht: ZendHashTable::new(),
        })
    }
    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<SeqSer<'a>, SerError> {
        Ok(SeqSer {
            ctx: self.ctx,
            ht: ZendHashTable::new(),
        })
    }
    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<TupleVariantSer<'a>, SerError> {
        Ok(TupleVariantSer {
            ctx: self.ctx,
            variant,
            ht: ZendHashTable::new(),
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<MapSer<'a>, SerError> {
        Ok(MapSer {
            ctx: self.ctx,
            ht: ZendHashTable::new(),
            pending_key: None,
        })
    }

    fn serialize_struct(self, name: &'static str, _len: usize) -> Result<StructSer<'a>, SerError> {
        let mut ht = ZendHashTable::new();
        let is_image = name == "Image";
        let img_id = if is_image {
            let id = self.ctx.image_counter;
            self.ctx.image_counter += 1;
            let mut z = Zval::new();
            z.set_long(id);
            let _ = ht.insert("image_id", z);
            id
        } else {
            0
        };
        Ok(StructSer {
            ctx: self.ctx,
            ht,
            is_image,
            img_id,
            img_format: None,
            img_bytes: None,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<StructVariantSer<'a>, SerError> {
        Ok(StructVariantSer {
            ctx: self.ctx,
            variant,
            ht: ZendHashTable::new(),
        })
    }
}

/// Sequence / tuple / tuple-struct builder → a packed (list) PHP array.
pub struct SeqSer<'a> {
    ctx: &'a mut Context,
    ht: ext_php_rs::boxed::ZBox<ZendHashTable>,
}

impl<'a> SerializeSeq for SeqSer<'a> {
    type Ok = Zval;
    type Error = SerError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        let z = value.serialize(ZvalSerializer { ctx: self.ctx })?;
        let _ = self.ht.push(z);
        Ok(())
    }
    fn end(self) -> Result<Zval, SerError> {
        Ok(ht_into_zval(self.ht))
    }
}

impl<'a> SerializeTuple for SeqSer<'a> {
    type Ok = Zval;
    type Error = SerError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Zval, SerError> {
        SerializeSeq::end(self)
    }
}

impl<'a> SerializeTupleStruct for SeqSer<'a> {
    type Ok = Zval;
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Zval, SerError> {
        SerializeSeq::end(self)
    }
}

/// Externally-tagged tuple variant → `{ variant: [items] }`.
pub struct TupleVariantSer<'a> {
    ctx: &'a mut Context,
    variant: &'static str,
    ht: ext_php_rs::boxed::ZBox<ZendHashTable>,
}

impl<'a> SerializeTupleVariant for TupleVariantSer<'a> {
    type Ok = Zval;
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        let z = value.serialize(ZvalSerializer { ctx: self.ctx })?;
        let _ = self.ht.push(z);
        Ok(())
    }
    fn end(self) -> Result<Zval, SerError> {
        let inner = ht_into_zval(self.ht);
        let mut outer = ZendHashTable::new();
        let _ = outer.insert(self.variant, inner);
        Ok(ht_into_zval(outer))
    }
}

/// Map builder → an associative PHP array. Defensive: the IR uses no maps.
pub struct MapSer<'a> {
    ctx: &'a mut Context,
    ht: ext_php_rs::boxed::ZBox<ZendHashTable>,
    pending_key: Option<String>,
}

impl<'a> SerializeMap for MapSer<'a> {
    type Ok = Zval;
    type Error = SerError;
    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result<(), SerError> {
        self.pending_key = Some(key.serialize(KeyCapture)?);
        Ok(())
    }
    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        let z = value.serialize(ZvalSerializer { ctx: self.ctx })?;
        let key = self.pending_key.take().unwrap_or_default();
        let _ = self.ht.insert(key.as_str(), z);
        Ok(())
    }
    fn end(self) -> Result<Zval, SerError> {
        Ok(ht_into_zval(self.ht))
    }
}

/// Struct builder → an associative PHP array, with image interception.
pub struct StructSer<'a> {
    ctx: &'a mut Context,
    ht: ext_php_rs::boxed::ZBox<ZendHashTable>,
    is_image: bool,
    img_id: i64,
    img_format: Option<String>,
    img_bytes: Option<Vec<u8>>,
}

impl<'a> SerializeStruct for StructSer<'a> {
    type Ok = Zval;
    type Error = SerError;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        if self.is_image && key == "data" {
            // Only pull the bytes out of the u8 stream if this call actually
            // needs them (getIr(false) needs none). This transient Vec is the
            // *only* copy of this image's bytes we hold, and it lives just until
            // this struct's end() — we never accumulate every image's Vec.
            if self.ctx.needs_image_bytes() {
                let bytes = value.serialize(ByteCapture)?;
                // Inline into the tree as a binary string when opted in
                // (getIr(true)).
                if self.ctx.inline_image_data {
                    let z = match &bytes {
                        Some(b) => binary_string_zval(b),
                        None => null_zval(),
                    };
                    let _ = self.ht.insert("data", z);
                }
                self.img_bytes = bytes;
            }
            return Ok(());
        }
        if self.is_image && key == "format" {
            // Capture the format string for the side-channel and mirror it into
            // the tree.
            let fmt = value.serialize(StringCapture)?;
            let z = match &fmt {
                Some(s) => {
                    let mut z = Zval::new();
                    let _ = z.set_string(s, false);
                    z
                }
                None => null_zval(),
            };
            let _ = self.ht.insert("format", z);
            self.img_format = fmt;
            return Ok(());
        }
        let z = value.serialize(ZvalSerializer { ctx: self.ctx })?;
        let _ = self.ht.insert(key, z);
        Ok(())
    }

    fn end(self) -> Result<Zval, SerError> {
        if self.is_image {
            // Materialise this image's final form now, so its raw bytes can be
            // dropped immediately — the side-channel only ever holds finished
            // PHP values, not a growing pile of Vec<u8>.
            let value = match &self.ctx.sink {
                ImageSink::None => None,
                ImageSink::Binary => self.img_bytes.as_deref().map(binary_string_zval),
                ImageSink::Directory(dir) => match &self.img_bytes {
                    Some(bytes) => {
                        let ext = self.img_format.as_deref().unwrap_or("bin");
                        let path = format!(
                            "{}/image_{}.{}",
                            dir.trim_end_matches('/'),
                            self.img_id,
                            ext
                        );
                        std::fs::write(&path, bytes)
                            .map_err(|e| SerError(format!("cannot write {path}: {e}")))?;
                        let mut z = Zval::new();
                        let _ = z.set_string(&path, false);
                        Some(z)
                    }
                    None => None,
                },
            };
            self.ctx.images.push(CollectedImage {
                id: self.img_id,
                format: self.img_format,
                value,
            });
        }
        Ok(ht_into_zval(self.ht))
    }
}

/// Externally-tagged struct variant → `{ variant: { fields } }`.
pub struct StructVariantSer<'a> {
    ctx: &'a mut Context,
    variant: &'static str,
    ht: ext_php_rs::boxed::ZBox<ZendHashTable>,
}

impl<'a> SerializeStructVariant for StructVariantSer<'a> {
    type Ok = Zval;
    type Error = SerError;
    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), SerError> {
        let z = value.serialize(ZvalSerializer { ctx: self.ctx })?;
        let _ = self.ht.insert(key, z);
        Ok(())
    }
    fn end(self) -> Result<Zval, SerError> {
        let inner = ht_into_zval(self.ht);
        let mut outer = ZendHashTable::new();
        let _ = outer.insert(self.variant, inner);
        Ok(ht_into_zval(outer))
    }
}

// ── Leaf capture serializers ────────────────────────────────────────────────
//
// These extract a single typed value from a sub-tree without building any
// Zval. `ByteCapture` pulls the raw bytes out of `Option<Vec<u8>>` (which
// serialises as an optional seq of u8); `StringCapture` pulls an optional
// string (used for `Image.format`, an `Option<ImageFormat>` unit-variant enum).

/// Extracts `Option<Vec<u8>>` from a value serialized as an optional u8 seq.
struct ByteCapture;

impl Serializer for ByteCapture {
    type Ok = Option<Vec<u8>>;
    type Error = SerError;
    type SerializeSeq = ByteSeq;
    type SerializeTuple = ByteSeq;
    type SerializeTupleStruct = ser::Impossible<Option<Vec<u8>>, SerError>;
    type SerializeTupleVariant = ser::Impossible<Option<Vec<u8>>, SerError>;
    type SerializeMap = ser::Impossible<Option<Vec<u8>>, SerError>;
    type SerializeStruct = ser::Impossible<Option<Vec<u8>>, SerError>;
    type SerializeStructVariant = ser::Impossible<Option<Vec<u8>>, SerError>;

    fn serialize_none(self) -> Result<Self::Ok, SerError> {
        Ok(None)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, SerError> {
        value.serialize(self)
    }
    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, SerError> {
        Ok(Some(v.to_vec()))
    }
    fn serialize_seq(self, len: Option<usize>) -> Result<ByteSeq, SerError> {
        Ok(ByteSeq(Vec::with_capacity(len.unwrap_or(0))))
    }
    fn serialize_tuple(self, len: usize) -> Result<ByteSeq, SerError> {
        Ok(ByteSeq(Vec::with_capacity(len)))
    }
    fn serialize_unit(self) -> Result<Self::Ok, SerError> {
        Ok(None)
    }

    fn serialize_bool(self, _: bool) -> Result<Self::Ok, SerError> {
        Err(unexpected("bool", "image data"))
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok, SerError> {
        Err(unexpected("i8", "image data"))
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok, SerError> {
        Err(unexpected("i16", "image data"))
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok, SerError> {
        Err(unexpected("i32", "image data"))
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok, SerError> {
        Err(unexpected("i64", "image data"))
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok, SerError> {
        Err(unexpected("u8", "image data"))
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok, SerError> {
        Err(unexpected("u16", "image data"))
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok, SerError> {
        Err(unexpected("u32", "image data"))
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok, SerError> {
        Err(unexpected("u64", "image data"))
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok, SerError> {
        Err(unexpected("f32", "image data"))
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok, SerError> {
        Err(unexpected("f64", "image data"))
    }
    fn serialize_char(self, _: char) -> Result<Self::Ok, SerError> {
        Err(unexpected("char", "image data"))
    }
    fn serialize_str(self, _: &str) -> Result<Self::Ok, SerError> {
        Err(unexpected("str", "image data"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, SerError> {
        Err(unexpected("unit struct", "image data"))
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Self::Ok, SerError> {
        Err(unexpected("unit variant", "image data"))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Self::Ok, SerError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, SerError> {
        Err(unexpected("newtype variant", "image data"))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, SerError> {
        Err(unexpected("tuple struct", "image data"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, SerError> {
        Err(unexpected("tuple variant", "image data"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, SerError> {
        Err(unexpected("map", "image data"))
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, SerError> {
        Err(unexpected("struct", "image data"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, SerError> {
        Err(unexpected("struct variant", "image data"))
    }
}

struct ByteSeq(Vec<u8>);

impl SerializeSeq for ByteSeq {
    type Ok = Option<Vec<u8>>;
    type Error = SerError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        self.0.push(value.serialize(U8Capture)?);
        Ok(())
    }
    fn end(self) -> Result<Self::Ok, SerError> {
        Ok(Some(self.0))
    }
}

impl SerializeTuple for ByteSeq {
    type Ok = Option<Vec<u8>>;
    type Error = SerError;
    fn serialize_element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result<(), SerError> {
        SerializeSeq::serialize_element(self, value)
    }
    fn end(self) -> Result<Self::Ok, SerError> {
        SerializeSeq::end(self)
    }
}

/// Extracts a single `u8` from a scalar element.
struct U8Capture;

impl Serializer for U8Capture {
    type Ok = u8;
    type Error = SerError;
    type SerializeSeq = ser::Impossible<u8, SerError>;
    type SerializeTuple = ser::Impossible<u8, SerError>;
    type SerializeTupleStruct = ser::Impossible<u8, SerError>;
    type SerializeTupleVariant = ser::Impossible<u8, SerError>;
    type SerializeMap = ser::Impossible<u8, SerError>;
    type SerializeStruct = ser::Impossible<u8, SerError>;
    type SerializeStructVariant = ser::Impossible<u8, SerError>;

    fn serialize_u8(self, v: u8) -> Result<u8, SerError> {
        Ok(v)
    }
    // Widen the other integer types defensively, clamping is not expected.
    fn serialize_u16(self, v: u16) -> Result<u8, SerError> {
        Ok(v as u8)
    }
    fn serialize_u32(self, v: u32) -> Result<u8, SerError> {
        Ok(v as u8)
    }
    fn serialize_u64(self, v: u64) -> Result<u8, SerError> {
        Ok(v as u8)
    }
    fn serialize_i8(self, v: i8) -> Result<u8, SerError> {
        Ok(v as u8)
    }
    fn serialize_i16(self, v: i16) -> Result<u8, SerError> {
        Ok(v as u8)
    }
    fn serialize_i32(self, v: i32) -> Result<u8, SerError> {
        Ok(v as u8)
    }
    fn serialize_i64(self, v: i64) -> Result<u8, SerError> {
        Ok(v as u8)
    }

    fn serialize_bool(self, _: bool) -> Result<u8, SerError> {
        Err(unexpected("bool", "image byte"))
    }
    fn serialize_f32(self, _: f32) -> Result<u8, SerError> {
        Err(unexpected("f32", "image byte"))
    }
    fn serialize_f64(self, _: f64) -> Result<u8, SerError> {
        Err(unexpected("f64", "image byte"))
    }
    fn serialize_char(self, _: char) -> Result<u8, SerError> {
        Err(unexpected("char", "image byte"))
    }
    fn serialize_str(self, _: &str) -> Result<u8, SerError> {
        Err(unexpected("str", "image byte"))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<u8, SerError> {
        Err(unexpected("bytes", "image byte"))
    }
    fn serialize_none(self) -> Result<u8, SerError> {
        Err(unexpected("none", "image byte"))
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<u8, SerError> {
        Err(unexpected("some", "image byte"))
    }
    fn serialize_unit(self) -> Result<u8, SerError> {
        Err(unexpected("unit", "image byte"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<u8, SerError> {
        Err(unexpected("unit struct", "image byte"))
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<u8, SerError> {
        Err(unexpected("unit variant", "image byte"))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<u8, SerError> {
        value.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<u8, SerError> {
        Err(unexpected("newtype variant", "image byte"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, SerError> {
        Err(unexpected("seq", "image byte"))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, SerError> {
        Err(unexpected("tuple", "image byte"))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, SerError> {
        Err(unexpected("tuple struct", "image byte"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, SerError> {
        Err(unexpected("tuple variant", "image byte"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, SerError> {
        Err(unexpected("map", "image byte"))
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, SerError> {
        Err(unexpected("struct", "image byte"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, SerError> {
        Err(unexpected("struct variant", "image byte"))
    }
}

/// Extracts `Option<String>` from a value serialized as an optional string or
/// unit-variant enum (e.g. `Option<ImageFormat>`).
struct StringCapture;

impl Serializer for StringCapture {
    type Ok = Option<String>;
    type Error = SerError;
    type SerializeSeq = ser::Impossible<Option<String>, SerError>;
    type SerializeTuple = ser::Impossible<Option<String>, SerError>;
    type SerializeTupleStruct = ser::Impossible<Option<String>, SerError>;
    type SerializeTupleVariant = ser::Impossible<Option<String>, SerError>;
    type SerializeMap = ser::Impossible<Option<String>, SerError>;
    type SerializeStruct = ser::Impossible<Option<String>, SerError>;
    type SerializeStructVariant = ser::Impossible<Option<String>, SerError>;

    fn serialize_none(self) -> Result<Self::Ok, SerError> {
        Ok(None)
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<Self::Ok, SerError> {
        value.serialize(self)
    }
    fn serialize_str(self, v: &str) -> Result<Self::Ok, SerError> {
        Ok(Some(v.to_string()))
    }
    fn serialize_unit(self) -> Result<Self::Ok, SerError> {
        Ok(None)
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, SerError> {
        Ok(Some(variant.to_string()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<Self::Ok, SerError> {
        value.serialize(self)
    }

    fn serialize_bool(self, _: bool) -> Result<Self::Ok, SerError> {
        Err(unexpected("bool", "format string"))
    }
    fn serialize_i8(self, _: i8) -> Result<Self::Ok, SerError> {
        Err(unexpected("i8", "format string"))
    }
    fn serialize_i16(self, _: i16) -> Result<Self::Ok, SerError> {
        Err(unexpected("i16", "format string"))
    }
    fn serialize_i32(self, _: i32) -> Result<Self::Ok, SerError> {
        Err(unexpected("i32", "format string"))
    }
    fn serialize_i64(self, _: i64) -> Result<Self::Ok, SerError> {
        Err(unexpected("i64", "format string"))
    }
    fn serialize_u8(self, _: u8) -> Result<Self::Ok, SerError> {
        Err(unexpected("u8", "format string"))
    }
    fn serialize_u16(self, _: u16) -> Result<Self::Ok, SerError> {
        Err(unexpected("u16", "format string"))
    }
    fn serialize_u32(self, _: u32) -> Result<Self::Ok, SerError> {
        Err(unexpected("u32", "format string"))
    }
    fn serialize_u64(self, _: u64) -> Result<Self::Ok, SerError> {
        Err(unexpected("u64", "format string"))
    }
    fn serialize_f32(self, _: f32) -> Result<Self::Ok, SerError> {
        Err(unexpected("f32", "format string"))
    }
    fn serialize_f64(self, _: f64) -> Result<Self::Ok, SerError> {
        Err(unexpected("f64", "format string"))
    }
    fn serialize_char(self, v: char) -> Result<Self::Ok, SerError> {
        Ok(Some(v.to_string()))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<Self::Ok, SerError> {
        Err(unexpected("bytes", "format string"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<Self::Ok, SerError> {
        Err(unexpected("unit struct", "format string"))
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<Self::Ok, SerError> {
        Err(unexpected("newtype variant", "format string"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, SerError> {
        Err(unexpected("seq", "format string"))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, SerError> {
        Err(unexpected("tuple", "format string"))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, SerError> {
        Err(unexpected("tuple struct", "format string"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, SerError> {
        Err(unexpected("tuple variant", "format string"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, SerError> {
        Err(unexpected("map", "format string"))
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, SerError> {
        Err(unexpected("struct", "format string"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, SerError> {
        Err(unexpected("struct variant", "format string"))
    }
}

/// Extracts a `String` map key from a scalar.
struct KeyCapture;

impl Serializer for KeyCapture {
    type Ok = String;
    type Error = SerError;
    type SerializeSeq = ser::Impossible<String, SerError>;
    type SerializeTuple = ser::Impossible<String, SerError>;
    type SerializeTupleStruct = ser::Impossible<String, SerError>;
    type SerializeTupleVariant = ser::Impossible<String, SerError>;
    type SerializeMap = ser::Impossible<String, SerError>;
    type SerializeStruct = ser::Impossible<String, SerError>;
    type SerializeStructVariant = ser::Impossible<String, SerError>;

    fn serialize_str(self, v: &str) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_bool(self, v: bool) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_i8(self, v: i8) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_i16(self, v: i16) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_i32(self, v: i32) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_i64(self, v: i64) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_u8(self, v: u8) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_u16(self, v: u16) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_u32(self, v: u32) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_u64(self, v: u64) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_f32(self, v: f32) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_f64(self, v: f64) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_char(self, v: char) -> Result<String, SerError> {
        Ok(v.to_string())
    }
    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> Result<String, SerError> {
        Ok(variant.to_string())
    }
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<String, SerError> {
        value.serialize(self)
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        value: &T,
    ) -> Result<String, SerError> {
        value.serialize(self)
    }

    fn serialize_bytes(self, _: &[u8]) -> Result<String, SerError> {
        Err(unexpected("bytes", "map key"))
    }
    fn serialize_none(self) -> Result<String, SerError> {
        Err(unexpected("none", "map key"))
    }
    fn serialize_unit(self) -> Result<String, SerError> {
        Err(unexpected("unit", "map key"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, SerError> {
        Err(unexpected("unit struct", "map key"))
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, SerError> {
        Err(unexpected("newtype variant", "map key"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, SerError> {
        Err(unexpected("seq", "map key"))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, SerError> {
        Err(unexpected("tuple", "map key"))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, SerError> {
        Err(unexpected("tuple struct", "map key"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, SerError> {
        Err(unexpected("tuple variant", "map key"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, SerError> {
        Err(unexpected("map", "map key"))
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, SerError> {
        Err(unexpected("struct", "map key"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, SerError> {
        Err(unexpected("struct variant", "map key"))
    }
}

fn unexpected(got: &str, wanted: &str) -> SerError {
    SerError(format!("unexpected {got} while capturing {wanted}"))
}
