<?php

/**
 * Regenerate tests/fixtures/with-image.docx: a minimal, valid Word2007 document
 * containing one MODERN DrawingML inline image (<w:drawing><wp:inline><a:blip>).
 *
 * office_oxide only extracts DrawingML images (not legacy VML), so this fixture
 * is hand-assembled with ZipArchive rather than via PHPWord (which emits VML).
 * Self-contained: needs only the zip + gd extensions.
 *
 * Usage: php tests/fixtures/make-with-image.php
 */

declare(strict_types=1);

$dir = __DIR__;

// A small, distinctive PNG embedded into the document.
$w = 64;
$h = 48;
$im = imagecreatetruecolor($w, $h);
$blue = imagecolorallocate($im, 30, 60, 200);
$red = imagecolorallocate($im, 220, 40, 40);
imagefilledrectangle($im, 0, 0, $w, $h, $blue);
imagefilledellipse($im, (int) ($w / 2), (int) ($h / 2), 40, 30, $red);
ob_start();
imagepng($im);
$imgBytes = (string) ob_get_clean();
imagedestroy($im);

$contentTypes = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    . '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    . '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    . '<Default Extension="xml" ContentType="application/xml"/>'
    . '<Default Extension="png" ContentType="image/png"/>'
    . '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    . '</Types>';

$rels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    . '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    . '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
    . '</Relationships>';

$docRels = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    . '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    . '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>'
    . '</Relationships>';

$document = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    . '<w:document '
    . 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" '
    . 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" '
    . 'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
    . 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
    . 'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">'
    . '<w:body>'
    . '<w:p><w:r><w:t xml:space="preserve">Intro paragraph before the image.</w:t></w:r></w:p>'
    . '<w:p><w:r><w:drawing>'
    . '<wp:inline distT="0" distB="0" distL="0" distR="0">'
    . '<wp:extent cx="609600" cy="457200"/>'
    . '<wp:docPr id="1" name="Picture 1" descr="Test logo"/>'
    . '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
    . '<pic:pic>'
    . '<pic:nvPicPr><pic:cNvPr id="1" name="image1.png" descr="Test logo"/><pic:cNvPicPr/></pic:nvPicPr>'
    . '<pic:blipFill><a:blip r:embed="rId1"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
    . '<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="609600" cy="457200"/></a:xfrm>'
    . '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>'
    . '</pic:pic>'
    . '</a:graphicData></a:graphic>'
    . '</wp:inline>'
    . '</w:drawing></w:r></w:p>'
    . '<w:p><w:r><w:t xml:space="preserve">Closing paragraph after the image.</w:t></w:r></w:p>'
    . '</w:body></w:document>';

$out = $dir . '/with-image.docx';
@unlink($out);
$zip = new ZipArchive();
if ($zip->open($out, ZipArchive::CREATE) !== true) {
    fwrite(STDERR, "cannot create {$out}\n");
    exit(1);
}
$zip->addFromString('[Content_Types].xml', $contentTypes);
$zip->addFromString('_rels/.rels', $rels);
$zip->addFromString('word/document.xml', $document);
$zip->addFromString('word/_rels/document.xml.rels', $docRels);
$zip->addFromString('word/media/image1.png', $imgBytes);
$zip->close();

printf("wrote %s (%d bytes, image %d bytes)\n", $out, filesize($out), strlen($imgBytes));
