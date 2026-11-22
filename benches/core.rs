use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

use mybot::agent::assembler::Assembler;
use mybot::protocol::{Message, estimate_tokens};
use mybot::providers::sse::SseDecoder;
use mybot::tools::edit_match::{Edit, apply_edits};
use mybot::tools::truncate::head_tail;

fn sse_body(events: usize) -> String {
    let mut body = String::with_capacity(events * 160);
    for index in 0..events {
        body.push_str("event: content_block_delta\n");
        body.push_str(&format!(
            "data: {{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"chunk {index}\"}}}}\n\n"
        ));
    }
    body
}

fn conversation(messages: usize) -> Vec<Message> {
    (0..messages)
        .map(|index| Message::user(format!("message {index} {}", "x".repeat(500))))
        .collect()
}

fn benches(c: &mut Criterion) {
    // SSE decode: parse a synthetic Anthropic stream in 4 KB chunks.
    let body = sse_body(2_000);
    let mut group = c.benchmark_group("sse_decode");
    group.throughput(Throughput::Bytes(body.len() as u64));
    group.bench_function("2000_events_4k_chunks", |b| {
        b.iter(|| {
            let mut decoder = SseDecoder::new();
            let mut events = 0;
            for chunk in body.as_bytes().chunks(4_096) {
                events += decoder.push(black_box(chunk)).len();
            }
            events += usize::from(decoder.finish().is_some());
            black_box(events);
        });
    });
    group.finish();

    // Token estimation over a large history.
    let messages = conversation(2_000);
    let mut group = c.benchmark_group("estimate_tokens");
    group.throughput(Throughput::Elements(messages.len() as u64));
    group.bench_function("2000_messages", |b| {
        b.iter(|| black_box(estimate_tokens(black_box(&messages))));
    });
    group.finish();

    // Assembling streamed tool-call deltas.
    c.bench_function("assembler_100_tool_calls", |b| {
        b.iter(|| {
            let mut assembler = Assembler::new();
            for index in 0..100 {
                assembler.on_tool_start(index, &format!("call_{index}"), "read");
                assembler.on_tool_args(
                    index,
                    "{\"path\":\"src/main.rs\",\"offset\":0,\"limit\":200}",
                );
            }
            black_box(assembler.finish().invocations.len());
        });
    });

    // Head+tail truncation of a 1 MB tool output.
    let text = "abcdefghij".repeat(100_000);
    let mut group = c.benchmark_group("head_tail");
    group.throughput(Throughput::Bytes(text.len() as u64));
    group.bench_function("1mb_to_8kb", |b| {
        b.iter(|| black_box(head_tail(black_box(&text), 8_000)));
    });
    group.finish();

    // Exact and fuzzy edit matching on a ~100 KB file.
    let mut original = "line of source code\n".repeat(5_000);
    original.push_str("UNIQUE_MARKER\n");
    let exact = [Edit {
        old: "UNIQUE_MARKER".into(),
        new: "REPLACED".into(),
        replace_all: false,
    }];
    let fuzzy = [Edit {
        old: "      UNIQUE_MARKER".into(),
        new: "REPLACED".into(),
        replace_all: false,
    }];

    c.bench_function("edit_match_exact_100kb", |b| {
        b.iter(|| black_box(apply_edits(black_box(&original), black_box(&exact))));
    });
    c.bench_function("edit_match_fuzzy_100kb", |b| {
        b.iter(|| black_box(apply_edits(black_box(&original), black_box(&fuzzy))));
    });
}

criterion_group!(benches_group, benches);
criterion_main!(benches_group);
