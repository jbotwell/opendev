//! Centralized color palette and box-drawing constants.
//!
//! Mirrors Python's `style_tokens.py` for consistent styling across the TUI.

use ratatui::style::Color;

pub const PRIMARY: Color = Color::Rgb(208, 212, 220);
pub const ACCENT: Color = Color::Rgb(130, 160, 255);
pub const SUBTLE: Color = Color::Rgb(154, 160, 172);
pub const SUCCESS: Color = Color::Rgb(106, 209, 143);
pub const ERROR: Color = Color::Rgb(255, 92, 87);
pub const WARNING: Color = Color::Rgb(255, 179, 71);
pub const BLUE_BRIGHT: Color = Color::Rgb(74, 158, 255);
pub const BLUE_PATH: Color = Color::Rgb(88, 166, 255);
pub const GOLD: Color = Color::Rgb(255, 215, 0);
pub const BORDER: Color = Color::Rgb(88, 88, 88);
pub const BORDER_ACCENT: Color = Color::Rgb(147, 147, 255);

// Semantic colors (from Python style_tokens.py)
pub const GREY: Color = Color::Rgb(122, 126, 134);
pub const THINKING_BG: Color = Color::Rgb(90, 94, 102);
pub const ORANGE: Color = Color::Rgb(255, 140, 0);
pub const GREEN_LIGHT: Color = Color::Rgb(137, 209, 133);
pub const GREEN_BRIGHT: Color = Color::Rgb(0, 255, 0);
pub const BLUE_TASK: Color = Color::Rgb(37, 150, 190);
pub const BLUE_LIGHT: Color = Color::Rgb(156, 207, 253);
pub const ORANGE_CAUTION: Color = Color::Rgb(255, 165, 0);
pub const CYAN: Color = Color::Rgb(0, 191, 255);
pub const DIM_GREY: Color = Color::Rgb(107, 114, 128);

// Thinking phases
pub const PHASE_THINKING: Color = Color::Rgb(90, 94, 102);
pub const PHASE_CRITIQUE: Color = Color::Rgb(255, 179, 71);
pub const PHASE_REFINEMENT: Color = Color::Rgb(0, 191, 255);

// Markdown heading colors
pub const HEADING_1: Color = Color::Rgb(200, 130, 255);
pub const HEADING_2: Color = Color::Rgb(0, 191, 255);
pub const HEADING_3: Color = Color::Rgb(255, 179, 71);
pub const CODE_FG: Color = Color::Rgb(106, 209, 143);
pub const CODE_BG: Color = Color::Rgb(30, 30, 30);
pub const BULLET: Color = Color::Rgb(0, 255, 0);

// Icons
pub const THINKING_ICON: &str = "\u{27e1}"; // ⟡

// Box-drawing characters (rounded)
pub const BOX_TL: &str = "\u{256d}";
pub const BOX_TR: &str = "\u{256e}";
pub const BOX_BL: &str = "\u{2570}";
pub const BOX_BR: &str = "\u{256f}";
pub const BOX_H: &str = "\u{2500}";
pub const BOX_V: &str = "\u{2502}";

// Icons
pub const TOOL_HEADER: &str = "\u{23fa}";
pub const INLINE_ARROW: &str = "\u{23bf}";
pub const RESULT_PREFIX: &str = "\u{23bf}  ";

/// Centralized indentation constants for conversation rendering.
/// All conversation line prefixes are defined here — never hardcode indent strings elsewhere.
pub struct Indent;

impl Indent {
    /// 2-space continuation for wrapped lines under a message (matches icon+space width)
    pub const CONT: &str = "  ";
    /// Tool result continuation lines (3 spaces to match "⎿  " visual width)
    pub const RESULT_CONT: &str = "   ";
}
