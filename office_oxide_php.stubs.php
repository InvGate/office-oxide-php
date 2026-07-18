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
     * @return array<string, mixed>
     */
    public function getIr(): array {}

    /**
     * The full structured intermediate representation serialized as a JSON
     * string. Equivalent to json_encode($doc->getIr()) but produced directly
     * by the native core.
     *
     * @return string
     */
    public function toJson(): string {}
}

/**
 * Thrown when a document cannot be read. Extends the built-in \Exception, so it
 * can be caught either specifically or as a plain \Exception.
 */
class OfficeException extends \Exception
{
}
