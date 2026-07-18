# API reference

The extension registers two classes in the `OfficeOxide` namespace:

- [`OfficeOxide\Document`](#class-officeoxidedocument) — a read-only document handle.
- [`OfficeOxide\OfficeException`](#class-officeoxideofficeexception) — thrown on read errors.

All classes are provided by the compiled extension at runtime. For IDE
autocompletion and static analysis, point your tooling at
[`office_oxide_php.stubs.php`](../office_oxide_php.stubs.php); it is never loaded
at runtime.

Supported formats: **DOCX, DOC, XLSX, XLS, PPTX, PPT**.

---

## Class `OfficeOxide\Document`

An immutable handle to a parsed document. You never construct it with `new`;
use one of the two static factories.

### `Document::open`

```php
public static function open(string $path): Document
```

Open a document from a filesystem path. The format is detected from the file
extension.

| Parameter | Type     | Description                          |
| --------- | -------- | ------------------------------------ |
| `$path`   | `string` | Path to the document on disk.        |

- **Returns** a `Document`.
- **Throws** `OfficeOxide\OfficeException` if the file cannot be read, is
  corrupt, or has an unsupported extension.

```php
use OfficeOxide\Document;

$doc = Document::open('/srv/uploads/report.docx');
echo $doc->getText();
```

### `Document::fromString`

```php
public static function fromString(string $data, string $format): Document
```

Open a document from an in-memory byte string — useful when the bytes come from
a database, an upload stream, or cloud storage rather than a file.

| Parameter | Type     | Description                                                             |
| --------- | -------- | ---------------------------------------------------------------------- |
| `$data`   | `string` | Raw document bytes (binary-safe, e.g. from `file_get_contents()`).     |
| `$format` | `string` | Format hint by extension: `"docx"`, `"doc"`, `"xlsx"`, `"xls"`, `"pptx"`, `"ppt"` (case-insensitive). |

- **Returns** a `Document`.
- **Throws** `OfficeOxide\OfficeException` if `$format` is unknown or the bytes
  cannot be parsed.

```php
use OfficeOxide\Document;

$bytes = $storage->read('report.docx');   // raw string of bytes
$doc   = Document::fromString($bytes, 'docx');
```

> The `$data` parameter is binary-safe: it is read as raw bytes, so ZIP-based
> formats (DOCX/XLSX/PPTX) and legacy binary formats (DOC/XLS/PPT) are handled
> correctly.

### `Document::getText`

```php
public function getText(): string
```

Extract the document's plain text. Never throws.

```php
$plain = $doc->getText();
```

### `Document::toHtml`

```php
public function toHtml(): string
```

Render the document as an HTML fragment (not a full `<html>` document). Never
throws.

```php
echo '<article>' . $doc->toHtml() . '</article>';
```

### `Document::toMarkdown`

```php
public function toMarkdown(): string
```

Render the document as Markdown. Never throws.

### `Document::getFormat`

```php
public function getFormat(): string
```

The detected format as a lowercase extension string: one of `"docx"`, `"doc"`,
`"xlsx"`, `"xls"`, `"pptx"`, `"ppt"`. Never throws.

```php
if ($doc->getFormat() === 'docx') { /* ... */ }
```

### `Document::getMetadata`

```php
public function getMetadata(): array
```

Document metadata as an associative array. See
[Metadata shape](#metadata-shape). Never throws in practice.

```php
$meta = $doc->getMetadata();
echo $meta['title'] ?? '(untitled)';
```

### `Document::getIr`

```php
public function getIr(): array
```

The full structured **intermediate representation (IR)** as a nested PHP array,
shaped as `['metadata' => [...], 'sections' => [...]]`. This is the richest view
of the document: block structure, inline runs, tables, lists, images, and
formatting. See [IR shape](#ir-shape).

```php
$ir = $doc->getIr();
foreach ($ir['sections'] as $section) {
    foreach ($section['elements'] as $element) {
        if ($element['type'] === 'paragraph') { /* ... */ }
    }
}
```

### `Document::toJson`

```php
public function toJson(): string
```

The same IR as [`getIr()`](#documentgetir), serialized to a JSON string by the
native core. Equivalent in content to `json_encode($doc->getIr())`, but produced
directly (handy for caching, logging, or forwarding to another service).

```php
file_put_contents('report.ir.json', $doc->toJson());
```

---

## Class `OfficeOxide\OfficeException`

```php
class OfficeException extends \Exception {}
```

Thrown by `Document::open()` and `Document::fromString()` when a document cannot
be read. Because it extends the built-in `\Exception`, you can catch it either
specifically or generically:

```php
use OfficeOxide\Document;
use OfficeOxide\OfficeException;

try {
    $doc = Document::open($path);
} catch (OfficeException $e) {
    // Specific: a document read problem.
    error_log("office read failed: {$e->getMessage()}");
} catch (\Throwable $e) {
    // Anything else.
}
```

The message carries the underlying reason (missing file, unsupported format,
malformed content, ...).

---

## Data shapes

### Units

Numeric geometry in the IR uses the document formats' native units:

| Suffix        | Unit                | Conversion                         |
| ------------- | ------------------- | ---------------------------------- |
| `_twips`      | twentieths of a point | 1 twip = 1/1440 inch; 1 pt = 20 twips |
| `_half_pt`    | half-points         | font size; e.g. `24` = 12 pt       |
| `_emu`        | English Metric Units | 914400 EMU = 1 inch                |

Colors are `[r, g, b]` arrays of three integers (0–255).

### Metadata shape

Returned by `getMetadata()`, and also present at `$ir['metadata']`. Absent
values are `null`.

```php
[
    'format'      => 'docx',   // string, always present
    'title'       => null,     // ?string
    'author'      => null,     // ?string
    'subject'     => null,     // ?string
    'keywords'    => [],        // string[]
    'created'     => null,     // ?string (ISO-8601)
    'modified'    => null,     // ?string (ISO-8601)
    'description' => null,     // ?string
]
```

### IR shape

`getIr()` returns:

```php
[
    'metadata' => [ /* Metadata shape, above */ ],
    'sections' => [ /* Section, Section, ... */ ],
]
```

A **section** is a logical division — a Word section break, an Excel worksheet,
or a PowerPoint slide:

```php
[
    'title'       => null,          // ?string (e.g. slide title / sheet name)
    'break_type'  => 'continuous',  // section break kind
    'columns'     => null,          // ?int column count
    'elements'    => [ /* Element, Element, ... */ ],
    'page_setup'  => null,          // ?object page geometry
    'header'      => null,          // ?object (also first_page_/even_page_ variants)
    'footer'      => null,          // ?object (also first_page_/even_page_ variants)
]
```

#### Elements (block-level)

Every element is an object with a `"type"` discriminator. **Switch on `type`**,
and handle unknown types defensively — the set is open and may grow in future
`office_oxide` releases:

| `type`            | Meaning                          | Notable keys                                             |
| ----------------- | -------------------------------- | -------------------------------------------------------- |
| `paragraph`       | A paragraph of inline content    | `content` (inline[]), `alignment`, `*_twips` spacing/indent |
| `heading`         | A heading                        | `level` (1–6), `content` (inline[])                      |
| `table`           | A table                          | `rows`, `column_widths_twips`, `border`, `caption`       |
| `list`            | An ordered/unordered list        | `ordered` (bool), `items`, `start_number`, `level`       |
| `image`           | An embedded image                | `alt_text`, `data`, `format`, `display_*_emu`, `pixel_*` |
| `code_block`      | Preformatted code                | text content                                             |
| `thematic_break`  | Horizontal rule                  | — (no payload)                                            |
| `page_break`      | Hard page break                  | — (no payload)                                            |
| `column_break`    | Column break                     | — (no payload)                                            |
| `text_box`        | Floating/anchored text box       | nested content + geometry                                |
| `footnote` / `endnote` | Note body                   | note content                                             |

A **paragraph**/**heading** `content` is an array of **inline** objects, again
`"type"`-tagged:

| `type`         | Meaning                        | Notable keys                                          |
| -------------- | ------------------------------ | ----------------------------------------------------- |
| `text`         | A styled run of text           | `text`, `bold`, `italic`, `underline`, `color`, `font_name`, `font_size_half_pt`, `highlight`, `hyperlink`, ... |
| `line_break`   | Line break within a paragraph  | — (no payload)                                         |
| `footnote_ref` / `endnote_ref` | Inline reference mark | reference id                                          |

A **table** row is `{ cells, is_header, height_twips, ... }`; a **cell** is
`{ content, col_span, row_span, background_color, vertical_align, text_align, ... }`
where `content` is itself an array of block-level **elements** (cells can contain
paragraphs, nested tables, etc.). A **list** `item` is
`{ content: Element[], nested: ?List }`.

> **Image bytes.** When present, an image element's `data` is the raw bytes
> serialized as a **JSON/PHP array of integers** (0–255), and `format` names the
> encoding (e.g. `"png"`). For image-heavy documents this can make `getIr()` /
> `toJson()` large — if you only need text or structure, prefer `getText()` or
> ignore `image` elements.

#### Worked example

```php
use OfficeOxide\Document;

$doc = Document::open('report.docx');
$ir  = $doc->getIr();

/** Collect the text of every paragraph, joining its inline runs. */
$paragraphs = [];
foreach ($ir['sections'] as $section) {
    foreach ($section['elements'] as $el) {
        if ($el['type'] !== 'paragraph') {
            continue;
        }
        $text = '';
        foreach ($el['content'] as $inline) {
            if (($inline['type'] ?? null) === 'text') {
                $text .= $inline['text'];
            }
        }
        if ($text !== '') {
            $paragraphs[] = $text;
        }
    }
}

echo implode("\n", $paragraphs);
```

> **Heading detection is source-dependent.** Whether a visually-bold "heading"
> becomes a `heading` element (with a `level`) or a plain `paragraph` depends on
> how the source file encodes it. If you need every heading regardless of
> encoding, also inspect paragraphs' `outline_level` (non-`null` on many styled
> headings) rather than relying solely on the `heading` element type.

---

## Supported formats

| Format | Extension | Kind             |
| ------ | --------- | ---------------- |
| Word (OOXML)   | `docx` | Word processing  |
| Word (legacy)  | `doc`  | Word processing  |
| Excel (OOXML)  | `xlsx` | Spreadsheet      |
| Excel (legacy) | `xls`  | Spreadsheet      |
| PowerPoint (OOXML)  | `pptx` | Presentation |
| PowerPoint (legacy) | `ppt`  | Presentation |

All formats expose the same `Document` API. `getText()`, `toHtml()`,
`toMarkdown()`, and the IR are populated per the source format's structure
(e.g. spreadsheet cells surface as tables; slides as sections).

---

## Notes & limitations

- **Read-only.** The extension reads and converts documents; it does not create,
  edit, or save them.
- **`#[non_exhaustive]` node types.** `type` values in the IR are an open set.
  Always include a `default`/fallback branch when switching on `type`.
- **Native memory.** Parsing happens in Rust, so `getText()`/IR work keeps PHP's
  `memory_get_peak_usage()` low; total process memory (RSS) reflects the real
  cost. See [`bench/`](../bench/README.md).
