//! Pure rendering primitives extracted from `nexal-tui`.
//!
//! This crate owns the text-transformation layer of the TUI: markdown
//! rendering, diff rendering, line wrapping/truncation, syntax highlighting,
//! terminal-palette handling, and the small `Renderable`/`Insets` helpers.
//!
//! It deliberately has no dependency on `nexal-tui`, `nexal-core`, or any
//! application state. Callers pass in the data they want rendered and get
//! back `ratatui` primitives.
//!
//! The `nexal-tui` crate re-exports every module here at its old crate-root
//! path so existing `crate::render::...`, `crate::markdown_render::...`,
//! etc. references continue to resolve unchanged.

pub mod color;
pub mod diff_render;
pub mod line_truncation;
pub mod live_wrap;
pub mod markdown;
pub mod markdown_render;
pub mod render;
pub mod terminal_palette;
pub mod text_formatting;
pub mod wrapping;
