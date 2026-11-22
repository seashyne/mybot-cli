---
title: Performance
permalink: /performance/
---

# Performance

Measurements taken on 2026-11-22 with the release profile (default Cargo
settings: `opt-level = 3`, no LTO, no codegen-units tuning).

## Environment

| | |
| --- | --- |
| CPU | Intel Core i5-13400F (16 threads) |
| OS | Linux 7.3.0-11-generic |
| Rust | rustc 1.100.0 (2026-11-09) |
| Cargo | 1.100.0 |
| Benchmark harness | criterion 0.8.2 |
| Build | `cargo build --release` |

The machine was otherwise idle, but no CPU pinning or frequency locking was
used. Treat the numbers as indicative rather than absolute.

## Microbenchmarks

Run with:

```bash
cargo bench --bench core
# one group only:
cargo bench --bench core -- sse_decode
```

Criterion uses 3 s warm-up and 5 s measurement per benchmark, 100 samples.

| Benchmark | Workload | Time |
| --- | --- | --- |
| `sse_decode/2000_events_4k_chunks` | 2,000 Anthropic SSE events, fed in 4 KB chunks | 489–495 µs (~480 MiB/s) |
| `estimate_tokens/2000_messages` | Token estimate over 2,000 messages (~1 MB) | 1.28 µs |
| `assembler_100_tool_calls` | Assemble 100 streamed tool calls | 62 µs |
| `head_tail/1mb_to_8kb` | Truncate 1 MB of tool output to 8 KB | 177 ns |
| `edit_match_exact_100kb` | Exact edit match in a ~100 KB file | 9.8 µs |
| `edit_match_fuzzy_100kb` | Whitespace-tolerant fallback, same file | 129 µs |

Notes:

- SSE decoding is not a throughput bottleneck: the parser handles roughly
  480 MiB/s, far above any provider stream.
- `head_tail` only copies the retained head and tail (8 KB), so its derived
  throughput number is not meaningful; the per-call latency is.
- Fuzzy edit matching scans every line and is about 13× slower than an exact
  match, which is why the `edit` tool tries exact matching first.
- Token estimation is cheap enough to call on every status update and before
  compaction.

## Process-level measurements

### Startup

50 runs of `mybot --help`, measured with Python `time.perf_counter()`:

| Metric | Value |
| --- | --- |
| Minimum | 0.88 ms |
| Median | 1.26 ms |
| Mean | 1.30 ms |
| Maximum | 2.66 ms |

This covers process start, dynamic linking, and clap argument parsing. It does
not include provider network latency.

### Memory

```bash
/usr/bin/time -v ./target/release/mybot --help
```

Maximum resident set size: **6,040 KB (~5.9 MiB)**.

The TUI and long sessions grow with the conversation history; `/compact`
replaces that history with a summary, so it also bounds memory and request size.

### Binary size

| Artifact | Bytes | Approx. |
| --- | --- | --- |
| `target/release/mybot` | 14,914,840 | 14.2 MiB |
| stripped copy | 11,539,472 | 11.0 MiB |

```bash
cargo build --release
ls -l target/release/mybot
cp target/release/mybot /tmp/mybot-stripped
strip /tmp/mybot-stripped
ls -l /tmp/mybot-stripped
```

## ARM64

The release build and the full test suite (148 tests) were verified on
`linux/arm64` through Docker/QEMU (clippy is verified on the host). See
[install.md](install.md) for the command. Benchmarks in this report were not
run under QEMU because emulation makes the numbers meaningless. Run
`cargo bench --bench core` on real ARM64 hardware (for example a Raspberry Pi
5) to get representative figures.

## Reproducing the startup and memory numbers

With `hyperfine` installed:

```bash
hyperfine --warmup 3 './target/release/mybot --help'
```

Without it, the Python measurement used here was:

```python
import statistics, subprocess, time

times = []
for _ in range(50):
    start = time.perf_counter()
    subprocess.run(["./target/release/mybot", "--help"],
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=True)
    times.append((time.perf_counter() - start) * 1000)

times.sort()
print(f"min {times[0]:.2f} ms | median {statistics.median(times):.2f} ms "
      f"| mean {statistics.mean(times):.2f} ms | max {times[-1]:.2f} ms")
```

## Interpreting the results

- The hot path for interactive use is streaming: SSE parse plus JSON
  accumulation. Both are in the microsecond range per event.
- File tools are dominated by I/O, not by matching or truncation.
- Startup is a few milliseconds, so `mybot exec` is suitable for scripts and
  pre-commit hooks where process overhead matters.
- The largest variable cost is the model request itself, followed by the size
  of the replayed history. Use `/compact` on long sessions to keep both down.
