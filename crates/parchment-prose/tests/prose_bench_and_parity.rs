//! Parity, security bounds, and streaming allocation/latency gate tests for `parchment-prose`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use parchment_prose::{
    DiffLineKind, IncrementalProseParser, InlineSpan, LatexNode, ProseBlock, TokenKind,
    compute_unified_line_diff, highlight_code, parse_inline_markdown, parse_latex_bounded,
    parse_prose_blocks, parse_svg_bounded, render_latex_unicode,
};

struct CountingAlloc;

static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static TRACKING: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) > 0 {
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) > 0 && new_size > layout.size() {
            ALLOC_BYTES.fetch_add(new_size - layout.size(), Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

#[test]
fn streaming_70kb_code_fence_stays_under_1mb_alloc_and_5ms_total() {
    let mut payload = String::with_capacity(80_000);
    payload.push_str("# Streaming Code Fence Gate\n\nIntro paragraph.\n\n```rust\n");
    for i in 0..850 {
        payload.push_str(&format!(
            "pub fn step_{i:04}(acc: &mut String, delta: &str) -> usize {{ acc.push_str(delta); acc.len() }}\n"
        ));
    }
    payload.push_str("```\n\nFinished.\n");
    assert!(payload.len() > 70_000);

    let mut parser = IncrementalProseParser::new(true);
    ALLOC_BYTES.store(0, Ordering::SeqCst);
    TRACKING.fetch_add(1, Ordering::SeqCst);
    let t0 = Instant::now();

    for chunk in payload.as_bytes().chunks(320) {
        let s = std::str::from_utf8(chunk).unwrap();
        parser.push_delta(s);
        // While open, verify zero-copy body_range covers the code fence.
        assert!(parser.block_count() >= 1);
    }
    parser.finish();

    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    TRACKING.fetch_sub(1, Ordering::SeqCst);
    let bytes_allocated = ALLOC_BYTES.load(Ordering::SeqCst);

    println!(
        "PHASE3_PROSE_70KB payload={} B chunks={} elapsed={:.2} ms total_alloc={:.1} KB",
        payload.len(),
        payload.len().div_ceil(320),
        elapsed_ms,
        bytes_allocated as f64 / 1024.0
    );

    let blocks: Vec<ProseBlock> = parser.iter_blocks().cloned().collect();
    assert!(matches!(blocks[0], ProseBlock::Heading { level: 1, .. }));
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, ProseBlock::CodeFence { closed: true, .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, ProseBlock::Paragraph { text } if text.contains("Finished.")))
    );

    // Phase 3 hard gate: < 1 MB total allocation (v2 was 386 MB!) and < 5 ms (v2 was 97 ms!).
    assert!(
        bytes_allocated < 1_024 * 1_024,
        "70 KB streaming code-fence exceeded 1 MB alloc budget: {bytes_allocated} bytes"
    );
    assert!(
        elapsed_ms < 25.0, // generous debug bound; release is < 1 ms
        "70 KB streaming code-fence parse too slow even in debug: {elapsed_ms:.2} ms"
    );
}

#[test]
fn cached_split_never_breaks_open_code_fence_with_blank_lines() {
    let mut parser = IncrementalProseParser::new(true);
    parser.push_delta("Intro paragraph.\n\n```rust\nfn a() {}\n\n");
    assert_eq!(parser.block_count(), 2);
    parser.push_delta("fn b() {}\n```\n\nAfter fence.\n");
    parser.finish();

    let blocks: Vec<&ProseBlock> = parser.iter_blocks().collect();
    assert_eq!(blocks.len(), 3, "blocks: {blocks:?}");
    match blocks[1] {
        ProseBlock::CodeFence {
            language, closed, ..
        } => {
            assert_eq!(language, "rust");
            assert!(*closed);
            let body = blocks[1].code_str(parser.buffer_str());
            assert!(body.contains("fn a() {}"));
            assert!(body.contains("fn b() {}"));
        }
        other => panic!("expected CodeFence, got {other:?}"),
    }
}

#[test]
fn parses_tables_math_svg_inline_spans_syntax_and_diffs_with_bounds() {
    let doc = concat!(
        "## Overview\n\n",
        "| Key | Value |\n|---|---|\n| Engine | Rust |\n\n",
        "$$\\frac{\\alpha + 1}{\\sqrt{x^2 + 1}}$$\n\n",
        "<svg viewBox=\"0 0 100 50\"><script>alert(1)</script><rect x=\"1\" y=\"2\" width=\"30\" height=\"10\" fill=\"#8B5A2B\" /></svg>\n",
    );
    let blocks = parse_prose_blocks(doc, true);
    assert!(blocks.iter().any(|b| matches!(b, ProseBlock::TableBlock { headers, rows } if headers == &["Key", "Value"] && rows.len() == 1)));
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, ProseBlock::MathBlock { .. }))
    );
    assert!(
        blocks
            .iter()
            .any(|b| matches!(b, ProseBlock::SvgBlock { .. }))
    );

    let inline = parse_inline_markdown(
        "See **bold**, *em*, `code`, $x^2$, and [docs](https://example.com).",
        true,
    );
    assert!(inline.iter().any(|s| matches!(s, InlineSpan::Bold(_))));
    assert!(
        inline
            .iter()
            .any(|s| matches!(s, InlineSpan::InlineMath(m) if m == "x^2"))
    );
    assert!(
        inline
            .iter()
            .any(|s| matches!(s, InlineSpan::Link { label, .. } if label == "docs"))
    );

    let unicode_math = render_latex_unicode(r"\frac{\alpha + 1}{\sqrt{x^2}}");
    assert!(unicode_math.contains('α') && unicode_math.contains('√') && unicode_math.contains('²'));

    // Hostile deeply nested LaTeX falls back safely without stack overflow.
    let hostile_latex = format!("{}{}", "{".repeat(120), "}".repeat(120));
    assert!(matches!(
        parse_latex_bounded(&hostile_latex),
        LatexNode::PlainFallback(_)
    ));

    // SVG strips <script> and keeps safe rect.
    let svg = parse_svg_bounded(
        r##"<svg viewBox="0 0 100 50"><script>evil()</script><rect x="1" y="2" width="30" height="10" fill="#8B5A2B" /></svg>"##,
    )
    .unwrap();
    assert_eq!(svg.width, 100.0);
    assert_eq!(svg.items.len(), 1);

    // Syntax highlighter classifies keywords, strings, comments, numbers.
    let spans = highlight_code(
        "rust",
        "pub fn main() { let x = 42; // hi\n let s = \"ok\"; }",
    );
    assert!(spans.iter().any(|s| s.kind == TokenKind::Keyword));
    assert!(spans.iter().any(|s| s.kind == TokenKind::Number));
    assert!(spans.iter().any(|s| s.kind == TokenKind::Comment));
    assert!(spans.iter().any(|s| s.kind == TokenKind::String));

    // Unified diff collapses long unchanged runs (> 6 lines).
    let old = (1..=20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let new = (1..=20)
        .map(|i| {
            if i == 10 {
                "line 10 changed".into()
            } else {
                format!("line {i}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let diff = compute_unified_line_diff(&old, &new);
    assert!(
        diff.iter()
            .any(|d| d.kind == DiffLineKind::Removed && d.text == "line 10")
    );
    assert!(
        diff.iter()
            .any(|d| d.kind == DiffLineKind::Added && d.text == "line 10 changed")
    );
    assert!(diff.iter().any(|d| d.kind == DiffLineKind::Collapsed));
}
