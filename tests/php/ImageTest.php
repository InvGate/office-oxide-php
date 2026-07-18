<?php

declare(strict_types=1);

namespace OfficeOxide\Tests;

use OfficeOxide\Document;
use PHPUnit\Framework\TestCase;

/**
 * Image-handling tests for the office_oxide_php extension.
 *
 * Uses the `with-image.docx` fixture — a minimal Word2007 document with one
 * modern DrawingML inline PNG (regenerate via tests/fixtures/make-with-image.php).
 *
 * Run with the extension loaded, e.g.:
 *   php -d extension=target/release/liboffice_oxide_php.so vendor/bin/phpunit
 */
final class ImageTest extends TestCase
{
    private string $docx;

    /** The exact PNG bytes embedded in the fixture, read from its own zip. */
    private string $expectedPng;

    protected function setUp(): void
    {
        if (!extension_loaded('office_oxide_php')) {
            self::markTestSkipped(
                'office_oxide_php extension is not loaded; run phpunit with -d extension=<path-to-.so/.dll>'
            );
        }

        $dir = defined('OFFICE_OXIDE_FIXTURES') ? OFFICE_OXIDE_FIXTURES : __DIR__ . '/../fixtures';
        $this->docx = $dir . '/with-image.docx';
        self::assertFileExists($this->docx, 'fixture with-image.docx missing');

        $zip = new \ZipArchive();
        self::assertTrue($zip->open($this->docx) === true, 'cannot open fixture zip');
        $png = $zip->getFromName('word/media/image1.png');
        $zip->close();
        self::assertIsString($png, 'fixture is missing its embedded PNG');
        $this->expectedPng = $png;
    }

    public function testGetImagesReturnsExactBytes(): void
    {
        $images = Document::open($this->docx)->getImages();

        self::assertCount(1, $images);
        $img = $images[0];
        self::assertSame(0, $img['image_id']);
        self::assertSame('png', $img['format']);
        self::assertIsString($img['data']);
        // Byte-exact: no int-array bloat, no lossy conversion.
        self::assertSame($this->expectedPng, $img['data']);
        self::assertSame(strlen($this->expectedPng), strlen($img['data']));
    }

    public function testGetIrOmitsImageBytesByDefault(): void
    {
        $ir = Document::open($this->docx)->getIr();
        $image = $this->firstImageElement($ir);

        self::assertNotNull($image, 'expected an image element in the IR');
        self::assertArrayHasKey('image_id', $image);
        self::assertSame(0, $image['image_id']);
        self::assertArrayNotHasKey('data', $image, 'image bytes must be stripped by default');
    }

    public function testGetIrCanIncludeImageBytesAsBinaryString(): void
    {
        $ir = Document::open($this->docx)->getIr(include_image_data: true);
        $image = $this->firstImageElement($ir);

        self::assertNotNull($image);
        self::assertArrayHasKey('data', $image);
        self::assertSame($this->expectedPng, $image['data']);
    }

    public function testImageIdCorrelatesBetweenGetIrAndGetImages(): void
    {
        $doc = Document::open($this->docx);
        $image = $this->firstImageElement($doc->getIr());
        $images = $doc->getImages();

        self::assertSame($image['image_id'], $images[0]['image_id']);
    }

    public function testToJsonOmitsImageBytesByDefault(): void
    {
        $json = Document::open($this->docx)->toJson();
        self::assertJson($json);
        $image = $this->firstImageElement(json_decode($json, true));

        self::assertNotNull($image);
        self::assertSame(0, $image['image_id']);
        self::assertArrayNotHasKey('data', $image);
    }

    public function testToJsonIncludesImageBytesAsBase64(): void
    {
        $json = Document::open($this->docx)->toJson(include_image_data: true);
        self::assertJson($json);
        $image = $this->firstImageElement(json_decode($json, true));

        self::assertNotNull($image);
        self::assertArrayHasKey('data', $image);
        self::assertIsString($image['data']);
        self::assertSame($this->expectedPng, base64_decode($image['data'], true));
    }

    public function testGetImagesWritesFilesWhenGivenDirectory(): void
    {
        $dir = sys_get_temp_dir() . '/oxide-img-' . bin2hex(random_bytes(6));
        try {
            $images = Document::open($this->docx)->getImages($dir);

            self::assertCount(1, $images);
            $img = $images[0];
            self::assertSame(0, $img['image_id']);
            self::assertArrayHasKey('path', $img);
            self::assertArrayNotHasKey('data', $img, 'directory mode must not also return bytes');
            self::assertFileExists($img['path']);
            self::assertSame($this->expectedPng, file_get_contents($img['path']));
            self::assertStringEndsWith('image_0.png', $img['path']);
        } finally {
            if (isset($img['path']) && is_file($img['path'])) {
                unlink($img['path']);
            }
            if (is_dir($dir)) {
                rmdir($dir);
            }
        }
    }

    /** @param array<string, mixed> $node */
    private function firstImageElement(array $node): ?array
    {
        if (($node['type'] ?? null) === 'image') {
            return $node;
        }
        foreach ($node as $value) {
            if (is_array($value)) {
                $found = $this->firstImageElement($value);
                if ($found !== null) {
                    return $found;
                }
            }
        }
        return null;
    }
}
