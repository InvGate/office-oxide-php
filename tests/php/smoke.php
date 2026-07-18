<?php

/**
 * End-to-end smoke test for the office_oxide_php extension.
 *
 * Loads the compiled extension (via `php -d extension=... smoke.php`) and
 * exercises every public method of OfficeOxide\Document against a fixture
 * DOCX, asserting the results. Exits non-zero on the first failure so it can
 * gate CI.
 *
 * Usage:
 *   php -d extension=/path/to/liboffice_oxide_php.so tests/php/smoke.php
 */

declare(strict_types=1);

$failures = 0;

function check(string $label, bool $ok, string $detail = ''): void
{
    global $failures;
    if ($ok) {
        echo "  ok   - {$label}\n";
    } else {
        $failures++;
        echo "  FAIL - {$label}" . ($detail !== '' ? ": {$detail}" : '') . "\n";
    }
}

// --- Extension and symbols are present -------------------------------------
check('extension loaded', extension_loaded('office_oxide_php'));
check('Document class exists', class_exists(\OfficeOxide\Document::class));
check('OfficeException class exists', class_exists(\OfficeOxide\OfficeException::class));

$fixture = __DIR__ . '/../fixtures/sample.docx';
check('fixture exists', is_file($fixture), $fixture);

// --- open() + accessors ----------------------------------------------------
$doc = \OfficeOxide\Document::open($fixture);

$text = $doc->getText();
check('getText returns non-empty string', is_string($text) && $text !== '', var_export($text, true));
check('getText contains fixture content', str_contains($text, 'Hello from office_oxide_php'), $text);

check('getFormat is docx', $doc->getFormat() === 'docx', $doc->getFormat());

$html = $doc->toHtml();
check('toHtml returns non-empty string', is_string($html) && $html !== '');

$md = $doc->toMarkdown();
check('toMarkdown returns non-empty string', is_string($md) && $md !== '');

// --- structured IR ---------------------------------------------------------
$meta = $doc->getMetadata();
check('getMetadata returns array', is_array($meta));
check('metadata has format key', isset($meta['format']));

$ir = $doc->getIr();
check('getIr returns array', is_array($ir));
check('IR has metadata', isset($ir['metadata']) && is_array($ir['metadata']));
check('IR has sections list', isset($ir['sections']) && is_array($ir['sections']));

$json = $doc->toJson();
check('toJson returns string', is_string($json) && $json !== '');
$decoded = json_decode($json, true);
check('toJson is valid JSON', json_last_error() === JSON_ERROR_NONE);
check('decoded JSON matches getIr()', $decoded == $ir);

// --- fromString() ----------------------------------------------------------
$bytes = file_get_contents($fixture);
$doc2 = \OfficeOxide\Document::fromString($bytes, 'docx');
check('fromString getText matches open()', $doc2->getText() === $text);

// --- error handling --------------------------------------------------------
$threw = false;
try {
    \OfficeOxide\Document::open(__DIR__ . '/does-not-exist.docx');
} catch (\OfficeOxide\OfficeException $e) {
    $threw = true;
} catch (\Throwable $e) {
    // Also acceptable: it is a subclass of \Exception.
    $threw = true;
}
check('open() on missing file throws', $threw);

$threwFmt = false;
try {
    \OfficeOxide\Document::fromString('garbage', 'not-a-format');
} catch (\Throwable $e) {
    $threwFmt = true;
}
check('fromString() with bad format throws', $threwFmt);

// --- summary ---------------------------------------------------------------
echo "\n";
if ($failures === 0) {
    echo "All checks passed.\n";
    exit(0);
}
echo "{$failures} check(s) failed.\n";
exit(1);
