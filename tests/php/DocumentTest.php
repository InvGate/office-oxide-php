<?php

declare(strict_types=1);

namespace OfficeOxide\Tests;

use OfficeOxide\Document;
use OfficeOxide\OfficeException;
use PHPUnit\Framework\TestCase;

/**
 * Functional tests for the office_oxide_php extension.
 *
 * Run with the extension loaded, e.g.:
 *   php -d extension=target/release/liboffice_oxide_php.so vendor/bin/phpunit
 */
final class DocumentTest extends TestCase
{
    private string $docx;

    protected function setUp(): void
    {
        if (!extension_loaded('office_oxide_php')) {
            self::markTestSkipped(
                'office_oxide_php extension is not loaded; run phpunit with -d extension=<path-to-.so/.dll>'
            );
        }

        $dir = defined('OFFICE_OXIDE_FIXTURES') ? OFFICE_OXIDE_FIXTURES : __DIR__ . '/../fixtures';
        $this->docx = $dir . '/sample.docx';
        self::assertFileExists($this->docx, 'fixture DOCX missing');
    }

    public function testClassesAreRegistered(): void
    {
        self::assertTrue(class_exists(Document::class));
        self::assertTrue(class_exists(OfficeException::class));
        self::assertTrue(is_subclass_of(OfficeException::class, \Exception::class));
    }

    public function testOpenExtractsText(): void
    {
        $doc = Document::open($this->docx);
        $text = $doc->getText();

        self::assertIsString($text);
        self::assertNotSame('', $text);
        self::assertStringContainsString('Hello from office_oxide_php', $text);
    }

    public function testGetFormat(): void
    {
        self::assertSame('docx', Document::open($this->docx)->getFormat());
    }

    public function testHtmlAndMarkdownRenderers(): void
    {
        $doc = Document::open($this->docx);

        $html = $doc->toHtml();
        self::assertIsString($html);
        self::assertNotSame('', $html);

        $md = $doc->toMarkdown();
        self::assertIsString($md);
        self::assertNotSame('', $md);
    }

    public function testGetMetadataShape(): void
    {
        $meta = Document::open($this->docx)->getMetadata();

        self::assertIsArray($meta);
        self::assertArrayHasKey('format', $meta);
        self::assertSame('docx', $meta['format']);
        self::assertArrayHasKey('keywords', $meta);
        self::assertIsArray($meta['keywords']);
    }

    public function testGetIrStructure(): void
    {
        $ir = Document::open($this->docx)->getIr();

        self::assertIsArray($ir);
        self::assertArrayHasKey('metadata', $ir);
        self::assertArrayHasKey('sections', $ir);
        self::assertIsArray($ir['sections']);
        self::assertNotEmpty($ir['sections']);

        $section = $ir['sections'][0];
        self::assertArrayHasKey('elements', $section);
        self::assertIsArray($section['elements']);

        // The fixture is prose, so the first element should be a paragraph
        // whose inline content includes a text span.
        $first = $section['elements'][0];
        self::assertSame('paragraph', $first['type']);
        self::assertArrayHasKey('content', $first);
        self::assertSame('text', $first['content'][0]['type']);
    }

    public function testToJsonMatchesGetIr(): void
    {
        $doc = Document::open($this->docx);

        $json = $doc->toJson();
        self::assertIsString($json);

        $decoded = json_decode($json, true);
        self::assertSame(JSON_ERROR_NONE, json_last_error());
        self::assertEquals($doc->getIr(), $decoded);
    }

    public function testFromStringMatchesOpen(): void
    {
        $bytes = file_get_contents($this->docx);
        self::assertNotFalse($bytes);

        $fromBytes = Document::fromString($bytes, 'docx');
        $fromPath = Document::open($this->docx);

        self::assertSame($fromPath->getText(), $fromBytes->getText());
    }

    public function testOpenMissingFileThrows(): void
    {
        $this->expectException(OfficeException::class);
        Document::open(__DIR__ . '/does-not-exist.docx');
    }

    public function testFromStringUnknownFormatThrows(): void
    {
        $this->expectException(OfficeException::class);
        Document::fromString('not a real document', 'xyz');
    }
}
