//! Incremental UTF-8 streaming text buffer, prefix-cached Markdown block & inline AST,
//! lightweight multi-language syntax highlighter, bounded LaTeX math AST, safe SVG scene-graph
//! parser, and unified diff engine.

pub mod buffer;
pub mod diff;
pub mod highlight;
pub mod latex;
pub mod markdown;
pub mod svg;

pub use buffer::{StreamingTextBuffer, normalize_line_endings};
pub use diff::{DiffLine, DiffLineKind, compute_unified_line_diff, parse_diff_fence};
pub use highlight::{HighlightSpan, TokenKind, highlight_code};
pub use latex::{LatexNode, parse_latex_bounded, render_latex_unicode};
pub use markdown::{
    IncrementalProseParser, InlineSpan, ProseBlock, parse_inline_markdown, parse_prose_blocks,
};
pub use svg::{Affine2D, SvgColor, SvgDocument, SvgDrawItem, SvgPaint, parse_svg_bounded};
