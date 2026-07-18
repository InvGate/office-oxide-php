# Benchmark: office_oxide_php vs phpoffice/phpword

Compares this extension against [`phpoffice/phpword`](https://github.com/PHPOffice/PHPWord)
on the most common real task: **load a DOCX and extract all its text**.

## Methodology

- **Same files, both engines.** Fixtures are generated *by PHPWord* (`gen_fixtures.php`)
  so every file is a standards-compliant Word2007 document that both engines read.
  Sizes range from 100 to 15,000 paragraphs (with headings and multi-run
  paragraphs, i.e. realistic prose).
- **Isolated processes.** Each `(engine, file)` measurement runs in its own PHP
  subprocess (`worker.php`). Peak RSS is a per-process high-water mark, so mixing
  engines in one process would contaminate the reading.
- **The extension is loaded only for the oxide worker**, never for the PHPWord
  worker — PHPWord is not charged for our shared library.
- **Two memory metrics.** `memory_get_peak_usage()` only sees PHP's Zend
  allocator; our native code allocates on the Rust heap, invisible to it. So the
  headline metric is **process peak RSS** (`/proc/self/status` `VmHWM`), which
  counts memory in *any* language. We also subtract a per-engine **baseline**
  (interpreter + autoloader + extension, no document work) to report the memory
  attributable to processing the document ("RSS+doc").
- **Timing.** Each engine warms up once, then runs N timed iterations
  (`hrtime`); we report the mean and the min.
- Both engines extract the same text to within 2% (`text-length ratio ≈ 0.98`;
  the difference is newline handling), confirming an apples-to-apples workload.

## Running it

```sh
# From the repo root:
cargo build --release
cd bench
composer install
php run.php ../target/release/liboffice_oxide_php.so
```

Fixtures are generated automatically on first run. Raw numbers are written to
`bench/results.json`.

## Results

Environment: PHP 8.4.23 (NTS), PHPWord 1.4.0, Linux x86_64, release build of the
extension. Your absolute numbers will vary by machine; the *ratios* are the point.

| Paragraphs | File size | Time — oxide | Time — PHPWord | **Speedup** | Mem/doc — oxide | Mem/doc — PHPWord | **Mem saving** |
| ---------: | --------: | -----------: | -------------: | ----------: | --------------: | ----------------: | -------------: |
|        100 |   7.8 KiB |     0.42 ms  |      23.9 ms   |   **~57×**  |      2.2 MiB    |       3.6 MiB     |    **~1.7×**   |
|      1,000 |  11.6 KiB |     1.92 ms  |     228 ms     |  **~119×**  |      5.3 MiB    |      21.1 MiB     |    **~4.0×**   |
|      5,000 |  27.7 KiB |     8.20 ms  |    1,253 ms    |  **~153×**  |     20.8 MiB    |      72.4 MiB     |    **~3.5×**   |
|     15,000 |  67.8 KiB |    23.6 ms   |    3,963 ms    |  **~168×**  |     55.1 MiB    |     196.6 MiB     |    **~3.6×**   |

### Takeaways

- **Time: 50–170× faster**, and the gap widens with document size — PHPWord's
  time grows faster than linearly (it builds a full mutable object model),
  whereas the extension scales roughly linearly.
- **Memory: 3.5–4× less** for any non-trivial document. On the 15k-paragraph
  file PHPWord peaks near 200 MiB of document memory vs ~55 MiB for the
  extension.
- **PHP heap pressure**: because extraction happens in Rust, the PHP-visible peak
  (`memory_get_peak_usage`) stays tiny (single-digit MiB) even on the largest
  file, where PHPWord needs ~140 MiB of Zend-allocator memory — relevant if you
  run close to `memory_limit`.

## Caveats & fairness notes

- PHPWord does far more than text extraction — it builds a fully navigable,
  **mutable** document model and can **write** files. This extension is
  read-only. If you need to *edit* documents in PHP, PHPWord and this extension
  are not substitutes. The benchmark measures the read/extract path they share.
- Numbers depend on hardware, PHP build, and opcache. Re-run locally with
  `php run.php <ext>` to get figures for your environment.
- Fixtures are prose-like DOCX. Documents dominated by tables, images, or
  deeply-nested structure may shift the ratios.
