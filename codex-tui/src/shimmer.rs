use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Span;

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn elapsed_since_start() -> Duration {
    let start = PROCESS_START.get_or_init(Instant::now);
    start.elapsed()
}

pub(crate) fn shimmer_spans(text: &str) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    // Use time-based sweep synchronized to process start.
    let padding = 10usize;
    let period = chars.len() + padding * 2;
    let sweep_seconds = 2.0f32;
    let pos_f =
        (elapsed_since_start().as_secs_f32() % sweep_seconds) / sweep_seconds * (period as f32);
    let pos = pos_f as usize;
    let has_true_color = supports_color::on_cached(supports_color::Stream::Stdout)
        .map(|level| level.has_16m)
        .unwrap_or(false);
    let band_half_width = 3.0;

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(chars.len());
    for (i, ch) in chars.iter().enumerate() {
        let i_pos = i as isize + padding as isize;
        let pos = pos as isize;
        let dist = (i_pos - pos).abs() as f32;

        let t = if dist <= band_half_width {
            let x = std::f32::consts::PI * (dist / band_half_width);
            0.5 * (1.0 + x.cos())
        } else {
            0.0
        };
        let brightness = 0.4 + 0.6 * t;
        let level = (brightness * 255.0).clamp(0.0, 255.0) as u8;
        let style = if has_true_color {
            // Allow custom RGB colors, as the implementation is thoughtfully
            // adjusting the level of the default foreground color.
            #[allow(clippy::disallowed_methods)]
            {
                Style::default()
                    .fg(Color::Rgb(level, level, level))
                    .add_modifier(Modifier::BOLD)
            }
        } else {
            color_for_level(level)
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    spans
}

fn color_for_level(level: u8) -> Style {
    // Tune thresholds so the edges of the shimmer band appear dim
    // in fallback mode (no true color support).
    if level < 160 {
        Style::default().add_modifier(Modifier::DIM)
    } else if level < 224 {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}


pub(crate) fn shimmer_spans_tinted(text: &str, tint: Color) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    // Use time-based sweep synchronized to process start.
    let padding = 10usize;
    let period = chars.len() + padding * 2;
    let sweep_seconds = 2.0f32;
    let pos_f =
        (elapsed_since_start().as_secs_f32() % sweep_seconds) / sweep_seconds * (period as f32);
    let pos = pos_f as usize;
    let has_true_color = supports_color::on_cached(supports_color::Stream::Stdout)
        .map(|level| level.has_16m)
        .unwrap_or(false);
    let band_half_width = 3.0;

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(chars.len());
    for (i, ch) in chars.iter().enumerate() {
        let i_pos = i as isize + padding as isize;
        let pos = pos as isize;
        let dist = (i_pos - pos).abs() as f32;

        let t = if dist <= band_half_width {
            let x = std::f32::consts::PI * (dist / band_half_width);
            0.5 * (1.0 + x.cos())
        } else {
            0.0
        };
        let brightness = 0.4 + 0.6 * t;
        let level = (brightness * 255.0).clamp(0.0, 255.0) as u8;
        let style = if has_true_color {
            match tint {
                Color::Rgb(r, g, b) => {
                    let scale = level as f32 / 255.0;
                    let r = (r as f32 * scale).round() as u8;
                    let g = (g as f32 * scale).round() as u8;
                    let b = (b as f32 * scale).round() as u8;
                    Style::default().fg(Color::Rgb(r, g, b))
                }
                Color::Cyan => Style::default().fg(Color::Rgb(0, level, level)),
                Color::Blue => Style::default().fg(Color::Rgb(0, 0, level)),
                Color::Green => Style::default().fg(Color::Rgb(0, level, 0)),
                Color::Red => Style::default().fg(Color::Rgb(level, 0, 0)),
                Color::Magenta => Style::default().fg(Color::Rgb(level, 0, level)),
                Color::Yellow => Style::default().fg(Color::Rgb(level, level, 0)),
                Color::White | Color::Gray => Style::default().fg(Color::Rgb(level, level, level)),
                Color::Black => Style::default().fg(Color::Rgb(0, 0, 0)),
                _ => Style::default().fg(Color::Cyan),
            }
            .add_modifier(Modifier::BOLD)
        } else {
            let base = Style::default().fg(tint);
            if level < 160 {
                base.add_modifier(Modifier::DIM)
            } else if level < 224 {
                base
            } else {
                base.add_modifier(Modifier::BOLD)
            }
        };
        spans.push(Span::styled(ch.to_string(), style));
    }
    spans
}
