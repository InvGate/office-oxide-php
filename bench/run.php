<?php

/**
 * Benchmark orchestrator: compares office_oxide_php against phpoffice/phpword on
 * DOCX text extraction, measuring wall-clock time and memory.
 *
 * Each (engine, file) measurement runs in its OWN php subprocess so that peak
 * RSS is clean. The office_oxide extension is loaded ONLY for the oxide worker,
 * never for the phpword worker, so PHPWord is not charged for our library.
 *
 * Usage:
 *   php run.php <path-to-office_oxide_php.so>
 */

declare(strict_types=1);

require __DIR__ . '/vendor/autoload.php';

$ext = $argv[1] ?? getenv('OFFICE_OXIDE_EXT') ?: '';
if ($ext === '' || !is_file($ext)) {
    fwrite(STDERR, "usage: php run.php <path-to-office_oxide_php.so>\n");
    fwrite(STDERR, "  (or set OFFICE_OXIDE_EXT). Build with: cargo build --release\n");
    exit(2);
}
$ext = realpath($ext);
$php = PHP_BINARY;
$dir = __DIR__;
$fixturesDir = $dir . '/fixtures';

// Fixture paragraph counts and how many timed iterations to run for each.
// Larger files get fewer iterations to keep total runtime bounded.
$plan = [
    100 => 30,
    1000 => 20,
    5000 => 8,
    15000 => 4,
];

// --- ensure fixtures exist -------------------------------------------------
$missing = false;
foreach (array_keys($plan) as $n) {
    if (!is_file("{$fixturesDir}/sample-{$n}.docx")) {
        $missing = true;
    }
}
if ($missing) {
    echo "Generating fixtures...\n";
    passthru(escapeshellarg($php) . ' ' . escapeshellarg("{$dir}/gen_fixtures.php") . ' ' . escapeshellarg($fixturesDir), $rc);
    if ($rc !== 0) {
        fwrite(STDERR, "fixture generation failed\n");
        exit(1);
    }
    echo "\n";
}

/**
 * Run one worker subprocess and decode its JSON result.
 * The extension is injected only for the oxide engine.
 */
function run_worker(string $php, string $dir, string $ext, string $engine, string $fileArg, int $iterations): array
{
    $cmd = escapeshellarg($php);
    if ($engine === 'oxide') {
        $cmd .= ' -d ' . escapeshellarg('extension=' . $ext);
    }
    $cmd .= ' ' . escapeshellarg("{$dir}/worker.php")
        . ' ' . escapeshellarg($engine)
        . ' ' . escapeshellarg($fileArg)
        . ' ' . $iterations;

    $out = shell_exec($cmd . ' 2>/dev/null');
    $data = json_decode((string) $out, true);
    if (!is_array($data)) {
        return ['ok' => false, 'error' => 'no/invalid output from worker', 'raw' => trim((string) $out)];
    }
    return $data;
}

$engines = ['oxide', 'phpword'];

// --- baselines -------------------------------------------------------------
echo "Measuring process baselines (no document work)...\n";
$baseline = [];
foreach ($engines as $engine) {
    $r = run_worker($php, $dir, $ext, $engine, '__baseline__', 0);
    $baseline[$engine] = $r['rss_peak_bytes'] ?? 0;
    printf("  %-8s baseline RSS: %6.1f MiB\n", $engine, ($baseline[$engine]) / 1048576);
}
echo "\n";

// --- measurements ----------------------------------------------------------
$results = [];
foreach ($plan as $n => $iterations) {
    $file = "{$fixturesDir}/sample-{$n}.docx";
    $sizeKiB = filesize($file) / 1024;
    echo "sample-{$n}.docx ({$sizeKiB} KiB), {$iterations} iterations/engine:\n";

    $row = ['paragraphs' => $n, 'file_kib' => round($sizeKiB, 1), 'engines' => []];
    foreach ($engines as $engine) {
        $r = run_worker($php, $dir, $ext, $engine, $file, $iterations);
        if (empty($r['ok'])) {
            printf("  %-8s ERROR: %s\n", $engine, $r['error'] ?? 'unknown');
            $row['engines'][$engine] = $r;
            continue;
        }
        $rssDelta = max(0, ($r['rss_peak_bytes'] ?? 0) - ($baseline[$engine] ?? 0));
        $r['rss_delta_bytes'] = $rssDelta;
        $row['engines'][$engine] = $r;
        printf(
            "  %-8s  time %8.2f ms (min %8.2f)   RSS peak %7.1f MiB   RSS+doc %7.1f MiB   PHP peak %7.1f MiB   text %d\n",
            $engine,
            $r['time_ms_mean'],
            $r['time_ms_min'],
            ($r['rss_peak_bytes'] ?? 0) / 1048576,
            $rssDelta / 1048576,
            ($r['php_peak_bytes'] ?? 0) / 1048576,
            $r['text_len'] ?? 0
        );
    }

    // Sanity: both engines should extract roughly the same amount of text.
    $ox = $row['engines']['oxide'] ?? [];
    $pw = $row['engines']['phpword'] ?? [];
    if (!empty($ox['ok']) && !empty($pw['ok'])) {
        $ratio = ($pw['text_len'] ?: 1) > 0 ? ($ox['text_len'] / max(1, $pw['text_len'])) : 0;
        printf("  (text-length ratio oxide/phpword: %.2f)\n", $ratio);
    }
    echo "\n";
    $results[] = $row;
}

// --- summary table ---------------------------------------------------------
echo str_repeat('=', 92) . "\n";
echo "SUMMARY — office_oxide_php vs phpoffice/phpword (DOCX text extraction)\n";
echo str_repeat('=', 92) . "\n";
printf("%-11s | %-22s | %-22s | %-10s | %-9s\n", 'paragraphs', 'time mean (ms)', 'RSS+doc (MiB)', 'speedup', 'mem save');
printf("%-11s | %-10s %-10s | %-10s %-10s | %-10s | %-9s\n", '', 'oxide', 'phpword', 'oxide', 'phpword', 'x faster', 'x less');
echo str_repeat('-', 92) . "\n";
foreach ($results as $row) {
    $ox = $row['engines']['oxide'] ?? [];
    $pw = $row['engines']['phpword'] ?? [];
    if (empty($ox['ok']) || empty($pw['ok'])) {
        printf("%-11s | (incomplete)\n", $row['paragraphs']);
        continue;
    }
    $speedup = $ox['time_ms_mean'] > 0 ? $pw['time_ms_mean'] / $ox['time_ms_mean'] : 0;
    $memSave = ($ox['rss_delta_bytes'] ?? 0) > 0 ? ($pw['rss_delta_bytes'] ?? 0) / $ox['rss_delta_bytes'] : 0;
    printf(
        "%-11s | %-10.2f %-10.2f | %-10.1f %-10.1f | %-10.1f | %-9.1f\n",
        $row['paragraphs'],
        $ox['time_ms_mean'],
        $pw['time_ms_mean'],
        ($ox['rss_delta_bytes'] ?? 0) / 1048576,
        ($pw['rss_delta_bytes'] ?? 0) / 1048576,
        $speedup,
        $memSave
    );
}
echo str_repeat('=', 92) . "\n";

// --- persist raw results ---------------------------------------------------
$payload = [
    'php_version' => PHP_VERSION,
    'extension' => $ext,
    'phpword_version' => class_exists(\Composer\InstalledVersions::class)
        ? \Composer\InstalledVersions::getPrettyVersion('phpoffice/phpword')
        : 'unknown',
    'baselines_bytes' => $baseline,
    'results' => $results,
];
file_put_contents("{$dir}/results.json", json_encode($payload, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES));
echo "Raw results written to bench/results.json\n";
