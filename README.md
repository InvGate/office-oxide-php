# office_oxide_php

A PHP extension for reading Microsoft Office documents — **DOCX, DOC, XLSX, XLS,
PPTX, and PPT** — written in Rust. It wraps the pure-Rust
[`office_oxide`](https://crates.io/crates/office_oxide) crate via
[`ext-php-rs`](https://github.com/davidcole1340/ext-php-rs), so there are no
C/C++ dependencies and no external binaries or services.

```php
use OfficeOxide\Document;

$doc = Document::open('report.docx');

echo $doc->getText();        // plain text
echo $doc->toHtml();         // HTML fragment
echo $doc->toMarkdown();     // Markdown

$meta = $doc->getMetadata(); // ['format' => 'docx', 'title' => ..., ...]
$ir   = $doc->getIr();       // full structured tree as a nested array
$json = $doc->toJson();      // the same tree, as a JSON string
```

## Status

Read-only. The extension exposes the document as plain text, HTML, Markdown, and
a structured intermediate representation (IR). Writing/editing is not exposed.

## Performance

On DOCX text extraction, the extension is **50–170× faster** and uses **3.5–4×
less memory** than [`phpoffice/phpword`](https://github.com/PHPOffice/PHPWord)
(PHP 8.4 NTS, PHPWord 1.4). The gap widens with document size.

| Paragraphs | Time — oxide | Time — PHPWord | Speedup | Mem/doc — oxide | Mem/doc — PHPWord | Mem saving |
| ---------: | -----------: | -------------: | ------: | --------------: | ----------------: | ---------: |
|      1,000 |     1.9 ms   |     228 ms     | ~119×   |      5.3 MiB    |      21.1 MiB     |   ~4.0×    |
|     15,000 |    23.6 ms   |    3,963 ms    | ~168×   |     55.1 MiB    |     196.6 MiB     |   ~3.6×    |

PHPWord builds a full mutable, writable document model; this extension is
read-only, so they are not substitutes if you need to *edit* documents — the
benchmark measures the read/extract path they share. Full methodology,
caveats, and a reproducible harness are in [`bench/`](bench/README.md).

## Installation (prebuilt binaries)

Each [GitHub Release](../../releases) ships prebuilt binaries built for
**PHP 8.4, non-thread-safe (NTS)**:

| Platform      | Asset                                                        | File                    |
| ------------- | ----------------------------------------------------------- | ----------------------- |
| Linux x86_64  | `office_oxide_php-<version>-linux-x86_64-php8.4-nts.tar.gz`  | `office_oxide_php.so`   |
| Windows x64   | `office_oxide_php-<version>-windows-x64-php8.4-nts.zip`      | `office_oxide_php.dll`  |

1. Download and extract the archive for your platform.
2. Move the extension into your PHP extension directory:
   - Find it with `php-config --extension-dir` (Linux) or check `php --ini`.
3. Enable it in your `php.ini`:

   ```ini
   ; Linux
   extension=office_oxide_php.so
   ; Windows
   extension=office_oxide_php.dll
   ```

   Or load it ad hoc without editing `php.ini`:

   ```sh
   php -d extension=/full/path/to/office_oxide_php.so your-script.php
   ```

4. Verify:

   ```sh
   php -m | grep office_oxide_php
   ```

> **Version must match.** A PHP extension binary is tied to the exact PHP
> minor version (8.4) and thread-safety flavour (NTS). Loading a mismatched
> binary will fail. Check yours with `php -v` and `php -i | grep 'Thread Safety'`.

### IDE support

The release archive includes `office_oxide_php.stubs.php`. It is **not** loaded
at runtime — point your IDE / static analyser at it so `OfficeOxide\Document`
autocompletes and type-checks.

## API

All methods live on `OfficeOxide\Document` and throw `OfficeOxide\OfficeException`
(a subclass of `\Exception`) on failure.

| Method                                        | Returns  | Description                                             |
| --------------------------------------------- | -------- | ------------------------------------------------------ |
| `Document::open(string $path)`                | `Document` | Open from a file path; format detected by extension. |
| `Document::fromString(string $bytes, string $format)` | `Document` | Open from raw bytes; `$format` is `"docx"`, `"doc"`, `"xlsx"`, `"xls"`, `"pptx"`, or `"ppt"`. |
| `$doc->getText()`                             | `string` | Plain-text extraction.                                 |
| `$doc->toHtml()`                              | `string` | HTML fragment.                                         |
| `$doc->toMarkdown()`                          | `string` | Markdown.                                              |
| `$doc->getFormat()`                           | `string` | Detected format, e.g. `"docx"`.                        |
| `$doc->getMetadata()`                         | `array`  | Title, author, subject, keywords, dates, format.       |
| `$doc->getIr()`                               | `array`  | Full structured IR: `['metadata' => ..., 'sections' => ...]`. |
| `$doc->toJson()`                              | `string` | The IR serialized as JSON.                             |

### Error handling

```php
use OfficeOxide\Document;
use OfficeOxide\OfficeException;

try {
    $doc = Document::open($path);
    echo $doc->getText();
} catch (OfficeException $e) {
    fwrite(STDERR, "Could not read document: {$e->getMessage()}\n");
}
```

## Building from source

Requirements:

- Rust (stable on Linux; **nightly on Windows** — required for the
  `abi_vectorcall` feature).
- PHP 8.4 with development headers (`php-config`, `phpize`).
- LLVM / libclang (for `ext-php-rs`'s bindgen step).

```sh
# Linux (Debian/Ubuntu)
sudo apt-get install -y php-dev llvm-dev libclang-dev clang

cargo build --release
# Linux artifact:   target/release/liboffice_oxide_php.so
# Windows artifact:  target/release/office_oxide_php.dll
```

Run the end-to-end smoke test against the freshly built extension:

```sh
php -d extension="$PWD/target/release/liboffice_oxide_php.so" tests/php/smoke.php
```

## How it works

`ext-php-rs` exposes a Rust struct to PHP as a class. `OfficeOxide\Document`
wraps an `office_oxide::Document`; each method delegates to one upstream call.
The structured IR is `serde`-serializable, so `getIr()`/`toJson()` serialize it
once and (for the array form) walk the JSON value into native PHP arrays — a
single conversion path instead of hand-mapping ~20 node types to PHP classes.

## Testing

Rust-side quality is enforced by `cargo clippy -D warnings`, `cargo fmt`, and the
supply-chain gate (`cargo audit`, `cargo deny`, `cargo machete`). The
extension's *behaviour* is covered by a PHPUnit suite that runs against the
compiled extension:

```sh
cargo build --release
composer install
php -d extension="$PWD/target/release/liboffice_oxide_php.so" vendor/bin/phpunit
```

A dependency-free smoke test is also available (no Composer required):

```sh
php -d extension="$PWD/target/release/liboffice_oxide_php.so" tests/php/smoke.php
```

## Continuous integration

`.github/workflows/ci.yml` (on every push to `main` and every PR):

- **`lint`** — `cargo fmt --check`, `cargo audit`, `cargo deny check`, `cargo machete`.
- **`rust`** — `cargo test` and `cargo clippy --all-targets -D warnings`.
- **`build-linux` / `build-windows`** — gated on `lint` + `rust`; build the
  release extension, run the smoke test and the PHPUnit suite against it, and
  upload the binary as an artifact.

`.github/workflows/release.yml` (on a `v*` tag; `workflow_dispatch` runs a
build-only dry run) builds and smoke-tests three binaries — Windows x64, Ubuntu
(glibc ~2.39), and EL8 (glibc 2.28, for RHEL/Rocky/Alma 8 & 9) — asserts the tag
matches the crate version, and publishes them to a GitHub Release alongside the
IDE stubs.

The workflows are the source of truth for the exact, reproducible build steps.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at
your option — matching the licensing of both `office_oxide` and `ext-php-rs`.
