<?php

/**
 * Benchmark worker: reads one DOCX with one engine, repeated N times, and emits
 * a single line of JSON with timing and memory measurements.
 *
 * It is deliberately run as its OWN process (one engine + one file per process)
 * because peak RSS is a per-process high-water mark — mixing engines in one
 * process would contaminate the memory reading.
 *
 * Usage:
 *   php [-d extension=...] worker.php <engine> <file> <iterations>
 *     engine      = "oxide" | "phpword"
 *
 * Output (stdout): {"engine":..,"file":..,"iterations":..,"ok":true,
 *                   "text_len":..,"time_ms_min":..,"time_ms_mean":..,
 *                   "php_peak_bytes":..,"rss_peak_bytes":..}
 */

declare(strict_types=1);

require __DIR__ . '/vendor/autoload.php';

use PhpOffice\PhpWord\IOFactory;

/** Peak resident set size of this process, in bytes (Linux /proc). */
function peak_rss_bytes(): int
{
    $status = @file_get_contents('/proc/self/status');
    if ($status !== false && preg_match('/VmHWM:\s+(\d+)\s+kB/', $status, $m)) {
        return (int) $m[1] * 1024;
    }
    // Fallback: getrusage ru_maxrss (KiB on Linux).
    $u = getrusage();
    return (int) ($u['ru_maxrss'] ?? 0) * 1024;
}

/** Extract all text from a PHPWord element tree (recursively). */
function phpword_text($element): string
{
    if (method_exists($element, 'getElements')) {
        $buf = '';
        foreach ($element->getElements() as $child) {
            $buf .= phpword_text($child) . "\n";
        }
        return $buf;
    }
    if (method_exists($element, 'getText')) {
        $t = $element->getText();
        return is_string($t) ? $t : '';
    }
    return '';
}

/** Read $file with the given engine, returning the extracted plain text. */
function read_with(string $engine, string $file): string
{
    if ($engine === 'oxide') {
        return \OfficeOxide\Document::open($file)->getText();
    }

    if ($engine === 'phpword') {
        $word = IOFactory::load($file, 'Word2007');
        $out = '';
        foreach ($word->getSections() as $section) {
            $out .= phpword_text($section);
        }
        return $out;
    }

    fwrite(STDERR, "unknown engine: {$engine}\n");
    exit(2);
}

// --- arguments -------------------------------------------------------------
$engine = $argv[1] ?? '';
$file = $argv[2] ?? '';
$iterations = max(1, (int) ($argv[3] ?? 5));

if (!in_array($engine, ['oxide', 'phpword'], true)) {
    fwrite(STDERR, "usage: worker.php <oxide|phpword> <file|__baseline__> <iterations>\n");
    exit(2);
}

// Baseline mode: report the process's resting peak RSS after the interpreter,
// autoloader, and (for oxide) the extension are loaded, but before any document
// work. Subtracting this isolates the memory cost of processing the document.
if ($file === '__baseline__') {
    echo json_encode([
        'engine' => $engine,
        'file' => '__baseline__',
        'ok' => true,
        'php_peak_bytes' => memory_get_peak_usage(true),
        'rss_peak_bytes' => peak_rss_bytes(),
    ]), "\n";
    exit(0);
}

if (!is_file($file)) {
    fwrite(STDERR, "file not found: {$file}\n");
    exit(2);
}

// --- warm up (populate opcache, autoloader, file cache) --------------------
try {
    $text = read_with($engine, $file);
} catch (\Throwable $e) {
    echo json_encode([
        'engine' => $engine,
        'file' => basename($file),
        'ok' => false,
        'error' => $e->getMessage(),
    ]), "\n";
    exit(1);
}

// --- timed runs ------------------------------------------------------------
$times = [];
for ($i = 0; $i < $iterations; $i++) {
    $start = hrtime(true);
    $text = read_with($engine, $file);
    $times[] = (hrtime(true) - $start) / 1e6; // ms

    // Drop references so each iteration reflects a single-document workload.
    unset($word);
    gc_collect_cycles();
}

sort($times);
$mean = array_sum($times) / count($times);

echo json_encode([
    'engine' => $engine,
    'file' => basename($file),
    'iterations' => $iterations,
    'ok' => true,
    'text_len' => strlen($text),
    'time_ms_min' => round($times[0], 3),
    'time_ms_mean' => round($mean, 3),
    'php_peak_bytes' => memory_get_peak_usage(true),
    'rss_peak_bytes' => peak_rss_bytes(),
]), "\n";
