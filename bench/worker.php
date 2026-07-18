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
 *   php [-d extension=...] worker.php <engine> <file> <iterations> [workload]
 *     engine      = "oxide" | "phpword"
 *     workload    = "text" (default) | "images" | "images_dir"
 *
 * The workloads are apples-to-apples:
 *   - "images":     BOTH engines return the raw image body bytes, held all at
 *                   once (oxide getImages(); phpword Image::getImageString()).
 *   - "images_dir": BOTH engines write every image to a scratch directory one at
 *                   a time, holding minimal memory (oxide getImages($dir);
 *                   phpword writes each getImageString() to a file).
 *
 * Output (stdout): {"engine":..,"file":..,"iterations":..,"ok":true,
 *                   "workload":..,"payload_len":..,"time_ms_min":..,
 *                   "time_ms_mean":..,"php_peak_bytes":..,"rss_peak_bytes":..}
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

/** Collect the raw image-body bytes from a PHPWord element tree (recursively)
 *  into $out, so ALL images are held in memory at once — matching oxide's
 *  getImages(), which returns every image in a single array. */
function phpword_collect_images($element, array &$out): void
{
    if ($element instanceof \PhpOffice\PhpWord\Element\Image) {
        $bytes = $element->getImageString();
        if (is_string($bytes)) {
            $out[] = $bytes;
        }
    }
    if (method_exists($element, 'getElements')) {
        foreach ($element->getElements() as $child) {
            phpword_collect_images($child, $out);
        }
    }
}

/**
 * Extract every embedded image's bytes with the given engine, holding them all
 * in memory simultaneously, and return the total byte count. Both engines
 * return the same raw bytes and retain all images at once, so the peak-memory
 * comparison is apples-to-apples.
 */
function read_images_with(string $engine, string $file): int
{
    if ($engine === 'oxide') {
        // getImages() returns every image's bytes in one array (all retained).
        $images = \OfficeOxide\Document::open($file)->getImages();
        $total = 0;
        foreach ($images as $img) {
            $total += is_string($img['data']) ? strlen($img['data']) : 0;
        }
        return $total;
    }

    if ($engine === 'phpword') {
        $word = IOFactory::load($file, 'Word2007');
        $all = [];
        foreach ($word->getSections() as $section) {
            phpword_collect_images($section, $all);
        }
        // $all holds every image's bytes at once, like oxide's getImages().
        return array_sum(array_map('strlen', $all));
    }

    fwrite(STDERR, "unknown engine: {$engine}\n");
    exit(2);
}

/** Write every embedded image to $dir one at a time (holding minimal memory),
 *  returning the number of images written. Both engines stream to disk. */
function write_images_with(string $engine, string $file, string $dir): int
{
    if ($engine === 'oxide') {
        // getImages($dir) writes each image as it walks and returns paths only.
        return count(\OfficeOxide\Document::open($file)->getImages($dir));
    }

    if ($engine === 'phpword') {
        $word = IOFactory::load($file, 'Word2007');
        $n = 0;
        foreach ($word->getSections() as $section) {
            phpword_write_images($section, $dir, $n);
        }
        return $n;
    }

    fwrite(STDERR, "unknown engine: {$engine}\n");
    exit(2);
}

/** Write each PHPWord image to $dir one at a time, without retaining bytes. */
function phpword_write_images($element, string $dir, int &$n): void
{
    if ($element instanceof \PhpOffice\PhpWord\Element\Image) {
        $bytes = $element->getImageString();
        if (is_string($bytes)) {
            file_put_contents("{$dir}/pw_{$n}.img", $bytes);
            $n++;
        }
    }
    if (method_exists($element, 'getElements')) {
        foreach ($element->getElements() as $child) {
            phpword_write_images($child, $dir, $n);
        }
    }
}

/** Run one workload iteration, returning a payload size to prevent dead-code
 *  elimination and to sanity-check both engines see the same data. */
function run_once(string $engine, string $file, string $workload, string $scratchDir): int
{
    switch ($workload) {
        case 'images':
            return read_images_with($engine, $file);
        case 'images_dir':
            return write_images_with($engine, $file, $scratchDir);
        default:
            return strlen(read_with($engine, $file));
    }
}

// --- arguments -------------------------------------------------------------
$engine = $argv[1] ?? '';
$file = $argv[2] ?? '';
$iterations = max(1, (int) ($argv[3] ?? 5));
$workload = $argv[4] ?? 'text';

if (!in_array($engine, ['oxide', 'phpword'], true)) {
    fwrite(STDERR, "usage: worker.php <oxide|phpword> <file|__baseline__> <iterations> [text|images]\n");
    exit(2);
}
if (!in_array($workload, ['text', 'images', 'images_dir'], true)) {
    fwrite(STDERR, "unknown workload: {$workload} (expected 'text', 'images', or 'images_dir')\n");
    exit(2);
}

// Scratch directory for the images_dir workload (its own dir per process).
$scratchDir = sys_get_temp_dir() . '/oxide-bench-' . $engine . '-' . getmypid();
if ($workload === 'images_dir') {
    @mkdir($scratchDir, 0777, true);
    register_shutdown_function(static function () use ($scratchDir): void {
        foreach (glob($scratchDir . '/*') ?: [] as $f) {
            @unlink($f);
        }
        @rmdir($scratchDir);
    });
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
    $payload = run_once($engine, $file, $workload, $scratchDir);
} catch (\Throwable $e) {
    echo json_encode([
        'engine' => $engine,
        'file' => basename($file),
        'workload' => $workload,
        'ok' => false,
        'error' => $e->getMessage(),
    ]), "\n";
    exit(1);
}

// --- timed runs ------------------------------------------------------------
$times = [];
for ($i = 0; $i < $iterations; $i++) {
    $start = hrtime(true);
    $payload = run_once($engine, $file, $workload, $scratchDir);
    $times[] = (hrtime(true) - $start) / 1e6; // ms

    // Drop references so each iteration reflects a single-document workload.
    gc_collect_cycles();
}

sort($times);
$mean = array_sum($times) / count($times);

echo json_encode([
    'engine' => $engine,
    'file' => basename($file),
    'iterations' => $iterations,
    'workload' => $workload,
    'ok' => true,
    'payload_len' => $payload,
    'time_ms_min' => round($times[0], 3),
    'time_ms_mean' => round($mean, 3),
    'php_peak_bytes' => memory_get_peak_usage(true),
    'rss_peak_bytes' => peak_rss_bytes(),
]), "\n";
