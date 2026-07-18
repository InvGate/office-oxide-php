<?php

/**
 * IDE stubs for the office_oxide_php extension.
 *
 * This file is NEVER loaded at runtime — the compiled extension provides the
 * real classes. It exists only so editors and static analysers (PhpStorm,
 * intelephense, PHPStan, ...) understand the API. Point your IDE at it or add
 * it to your project's stub paths.
 */

namespace OfficeOxide;

/**
 * A read-only handle to a Microsoft Office document (DOCX, DOC, XLSX, XLS,
 * PPTX, or PPT), backed by the native office_oxide Rust core.
 */
class Document
{
    /**
     * Open a document from a filesystem path. The format is detected from the
     * file extension.
     *
     * @param string $path Path to the document.
     * @return Document
     * @throws OfficeException If the file cannot be read or the format is unsupported.
     */
    public static function open(string $path): Document {}

    /**
     * Open a document from an in-memory byte string.
     *
     * @param string $data  Raw document bytes (e.g. from file_get_contents()).
     * @param string $format File-extension format hint: "docx", "doc", "xlsx",
     *                       "xls", "pptx", or "ppt".
     * @return Document
     * @throws OfficeException If the format is unknown or the bytes cannot be read.
     */
    public static function fromString(string $data, string $format): Document {}

    /**
     * Extract the document's plain text.
     *
     * @return string
     */
    public function getText(): string {}

    /**
     * Render the document as an HTML fragment.
     *
     * @return string
     */
    public function toHtml(): string {}

    /**
     * Render the document as Markdown.
     *
     * @return string
     */
    public function toMarkdown(): string {}

    /**
     * The document's format as a lowercase extension string, e.g. "docx".
     *
     * @return string
     */
    public function getFormat(): string {}

    /**
     * Document metadata (title, author, subject, keywords, created, modified,
     * description, format) as an associative array. Missing values are null.
     *
     * @return array<string, mixed>
     */
    public function getMetadata(): array {}

    /**
     * The full structured intermediate representation as a nested array,
     * shaped like `['metadata' => [...], 'sections' => [...]]`.
     *
     * Each image element carries an ordinal `image_id` (document order). Image
     * bytes are omitted by default to avoid a large memory blow-up; fetch them
     * with {@see Document::getImages()} (correlated by `image_id`). Pass
     * `$include_image_data = true` to inline the bytes as PHP binary strings.
     *
     * @param bool $include_image_data Inline image bytes as binary strings.
     * @return array<string, mixed>
     */
    public function getIr(bool $include_image_data = false): array {}

    /**
     * Every embedded image, as a list of associative arrays.
     *
     * Without $output_dir, each entry is
     * `['image_id' => int, 'format' => ?string, 'data' => ?string]` where
     * `data` is a raw PHP binary string (or null when no bytes were extracted).
     *
     * With $output_dir, each image is written to
     * `{$output_dir}/image_{image_id}.{ext}` and the entry becomes
     * `['image_id' => int, 'format' => ?string, 'path' => ?string]` instead —
     * so the caller never holds all image bytes in PHP memory at once. The
     * directory is created if missing.
     *
     * `image_id` matches the value on the corresponding element in getIr().
     *
     * @param string|null $output_dir Directory to write images into; when given,
     *                                entries carry `path` instead of `data`.
     * @return array<int, array<string, mixed>>
     * @throws OfficeException If an image cannot be written to $output_dir.
     */
    public function getImages(?string $output_dir = null): array {}

    /**
     * The full structured intermediate representation serialized as a JSON
     * string. Equivalent to json_encode($doc->getIr()) but produced directly
     * by the native core.
     *
     * Mirrors getIr(): image elements carry `image_id` and image bytes are
     * omitted by default. Because JSON cannot hold raw bytes, passing
     * `$include_image_data = true` encodes them as base64 strings.
     *
     * @param bool $include_image_data Encode image bytes as base64 in the JSON.
     * @return string
     */
    public function toJson(bool $include_image_data = false): string {}
}

/**
 * Thrown when a document cannot be read. Extends the built-in \Exception, so it
 * can be caught either specifically or as a plain \Exception.
 */
class OfficeException extends \Exception
{
}
