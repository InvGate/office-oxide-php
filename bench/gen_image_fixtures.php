<?php

/**
 * Generate image-heavy DOCX fixtures for the image-extraction benchmark.
 *
 * Each fixture embeds N copies of a moderately-sized PNG as MODERN DrawingML
 * inline images (<w:drawing><wp:inline><a:blip>), which BOTH engines read:
 * office_oxide extracts DrawingML (not legacy VML), and PHPWord's Word2007
 * reader loads the same media from the archive. Hand-assembled with ZipArchive
 * because PHPWord *writes* VML, which office_oxide would not read back.
 *
 * Usage: php gen_image_fixtures.php [outputDir]
 */

declare(strict_types=1);

$outDir = $argv[1] ?? __DIR__ . '/fixtures';
@mkdir($outDir, 0777, true);

/** Image counts to generate. */
$counts = [5, 25, 100];

// A ~photo-like PNG that does not compress to nothing, so per-image bytes are
// non-trivial and memory differences are visible. Deterministic content.
$w = 400;
$h = 300;
$im = imagecreatetruecolor($w, $h);
for ($y = 0; $y < $h; $y++) {
    for ($x = 0; $x < $w; $x++) {
        // A smooth gradient plus a cheap deterministic ripple — avoids flat
        // regions that would compress away, without needing randomness.
        $r = ($x * 255) / $w;
        $g = ($y * 255) / $h;
        $b = (($x * $y) % 256);
        imagesetpixel($im, $x, $y, imagecolorallocate($im, (int) $r, (int) $g, (int) $b));
    }
}
ob_start();
imagepng($im);
$png = (string) ob_get_clean();
imagedestroy($im);
printf("base image: %d bytes (%dx%d)\n", strlen($png), $w, $h);

$contentTypesHead = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    . '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    . '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    . '<Default Extension="xml" ContentType="application/xml"/>'
    . '<Default Extension="png" ContentType="image/png"/>'
    . '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    . '</Types>';

$rootRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    . '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    . '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
    . '</Relationships>';

/** One inline-image paragraph referencing relationship $rid. */
function image_paragraph(string $rid, int $n): string
{
    return '<w:p><w:r><w:drawing>'
        . '<wp:inline distT="0" distB="0" distL="0" distR="0">'
        . '<wp:extent cx="2743200" cy="2057400"/>'
        . '<wp:docPr id="' . $n . '" name="Picture ' . $n . '" descr="Benchmark image ' . $n . '"/>'
        . '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        . '<pic:pic>'
        . '<pic:nvPicPr><pic:cNvPr id="' . $n . '" name="image' . $n . '.png"/><pic:cNvPicPr/></pic:nvPicPr>'
        . '<pic:blipFill><a:blip r:embed="' . $rid . '"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
        . '<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="2743200" cy="2057400"/></a:xfrm>'
        . '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>'
        . '</pic:pic>'
        . '</a:graphicData></a:graphic>'
        . '</wp:inline>'
        . '</w:drawing></w:r></w:p>';
}

foreach ($counts as $count) {
    $body = '';
    $docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        . '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">';
    for ($i = 1; $i <= $count; $i++) {
        $rid = "rId{$i}";
        $body .= image_paragraph($rid, $i);
        $docRels .= '<Relationship Id="' . $rid . '" '
            . 'Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" '
            . 'Target="media/image' . $i . '.png"/>';
    }
    $docRels .= '</Relationships>';

    $document = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        . '<w:document '
        . 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
        . 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
        . 'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        . 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        . 'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        . '<w:body>' . $body . '</w:body></w:document>';

    $file = "{$outDir}/images-{$count}.docx";
    @unlink($file);
    $zip = new ZipArchive();
    $zip->open($file, ZipArchive::CREATE);
    $zip->addFromString('[Content_Types].xml', $contentTypesHead);
    $zip->addFromString('_rels/.rels', $rootRels);
    $zip->addFromString('word/document.xml', $document);
    $zip->addFromString('word/_rels/document.xml.rels', $docRels);
    for ($i = 1; $i <= $count; $i++) {
        $zip->addFromString("word/media/image{$i}.png", $png);
    }
    $zip->close();

    printf(
        "generated %-24s %8.1f KiB (%d images, %.1f KiB/image raw)\n",
        basename($file),
        filesize($file) / 1024,
        $count,
        strlen($png) / 1024
    );
}

echo "done.\n";
