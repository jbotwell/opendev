//! Tool call formatting helpers for conversation display.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::app::DisplayToolCall;
use crate::formatters::style_tokens;
use crate::formatters::tool_registry::format_tool_call_parts_with_wd;
use crate::widgets::spinner::{COMPLETED_CHAR, CONTINUATION_CHAR};

/// Format a tool call as a styled line with category color coding.
pub(super) fn format_tool_call(tc: &DisplayToolCall, working_dir: Option<&str>) -> Line<'static> {
    let (icon, icon_color) = if tc.success {
        (COMPLETED_CHAR, style_tokens::GREEN_BRIGHT)
    } else {
        (COMPLETED_CHAR, style_tokens::ERROR)
    };

    // For ask_user, display as "⏺ User answered Claude's questions:"
    if tc.name == "ask_user" {
        return Line::from(vec![
            Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
            Span::styled(
                "User answered Claude's questions:",
                Style::default()
                    .fg(style_tokens::PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
    }

    let (verb, arg) = format_tool_call_parts_with_wd(&tc.name, &tc.arguments, working_dir);

    Line::from(vec![
        Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
        Span::styled(
            verb,
            Style::default()
                .fg(style_tokens::PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {arg}"), Style::default().fg(style_tokens::SUBTLE)),
    ])
}

/// Format a nested tool call with ⎿ continuation indent (Python style).
pub(super) fn format_nested_tool_call(
    tc: &DisplayToolCall,
    _depth: usize,
    working_dir: Option<&str>,
) -> Line<'static> {
    let (icon, icon_color) = if tc.success {
        (COMPLETED_CHAR, style_tokens::GREEN_BRIGHT)
    } else {
        ('\u{2717}', style_tokens::ERROR) // ✗
    };

    let (verb, arg) = format_tool_call_parts_with_wd(&tc.name, &tc.arguments, working_dir);

    Line::from(vec![
        Span::styled(
            format!("  {CONTINUATION_CHAR}  "),
            Style::default().fg(style_tokens::GREY),
        ),
        Span::styled(format!("{icon} "), Style::default().fg(icon_color)),
        Span::styled(verb, Style::default().fg(style_tokens::SUBTLE)),
        Span::styled(format!(" {arg}"), Style::default().fg(style_tokens::GREY)),
    ])
}
