<?php

/**
 * Generate DOCX fixtures of varying sizes using PHPWord itself, so both engines
 * read identical, standards-compliant Word2007 files. Sizes are given as the
 * number of body paragraphs.
 *
 * Usage: php gen_fixtures.php [outputDir]
 */

declare(strict_types=1);

require __DIR__ . '/vendor/autoload.php';

use PhpOffice\PhpWord\PhpWord;
use PhpOffice\PhpWord\IOFactory;

$outDir = $argv[1] ?? __DIR__ . '/fixtures';
@mkdir($outDir, 0777, true);

/** Paragraph counts to generate. */
$sizes = [100, 1000, 5000, 15000];

$lorem = 'The quick brown fox jumps over the lazy dog while the sun sets behind '
    . 'the distant hills and a gentle breeze carries the scent of rain.';

foreach ($sizes as $count) {
    $word = new PhpWord();
    $section = $word->addSection();

    for ($i = 0; $i < $count; $i++) {
        // Every 25th paragraph is a heading, to give the document some structure.
        if ($i % 25 === 0) {
            $section->addTitle("Section heading number {$i}", 1);
            continue;
        }

        // A multi-run paragraph: plain + bold + plain, like real prose.
        $run = $section->addTextRun();
        $run->addText("Paragraph {$i}: ");
        $run->addText('an important point', ['bold' => true]);
        $run->addText(". {$lorem}");
    }

    $file = "{$outDir}/sample-{$count}.docx";
    IOFactory::createWriter($word, 'Word2007')->save($file);

    $bytes = filesize($file);
    printf("generated %-28s %8.1f KiB (%d paragraphs)\n", basename($file), $bytes / 1024, $count);

    // Free the object model before building the next (larger) document.
    $word->getSections(); // no-op access
    unset($word, $section);
    gc_collect_cycles();
}

echo "done.\n";
