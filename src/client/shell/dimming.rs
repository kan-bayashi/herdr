use super::*;
use crate::protocol::{color_to_u32, u32_to_color, CellData};
use crate::terminal_theme::TerminalTheme;
use ratatui::style::Color;

type Rgb = (u8, u8, u8);

fn terminal_theme_to_rgb(color: crate::terminal_theme::RgbColor) -> Rgb {
    (color.r, color.g, color.b)
}

/// Prepared once per composition/patch, without terminal locks or cell allocations.
pub(super) struct InactivePaneDimmer<'a> {
    theme: &'a TerminalTheme,
    toward: Option<Rgb>,
    default_fg: Option<Color>,
    strength: f32,
}

impl<'a> InactivePaneDimmer<'a> {
    pub(super) fn new(config: &ClientShellConfig, theme: &'a TerminalTheme) -> Self {
        Self::for_target(
            inactive_dim_target(&config.palette, *theme),
            theme,
            config.dim_inactive_panes_strength,
        )
    }

    fn for_target(target: Option<Rgb>, theme: &'a TerminalTheme, strength: f32) -> Self {
        let toward = target.map(|target| {
            if relative_luminance(target) < 0.5 {
                (0, 0, 0)
            } else {
                (255, 255, 255)
            }
        });
        let mut dimmer = Self {
            theme,
            toward,
            default_fg: None,
            strength,
        };
        dimmer.default_fg = theme
            .foreground
            .and_then(|fg| dimmer.color(Color::Rgb(fg.r, fg.g, fg.b)));
        dimmer
    }

    fn color(&self, color: Color) -> Option<Color> {
        let toward = self.toward?;
        let rgb = dim_color_to_rgb(color, self.theme)?;
        let (r, g, b) = mix_rgb(rgb, toward, self.strength);
        Some(Color::Rgb(r, g, b))
    }

    pub(super) fn cell(&self, cell: &mut CellData) {
        match self.color(u32_to_color(cell.fg)).or(self.default_fg) {
            Some(fg) => cell.fg = color_to_u32(fg),
            None => cell.modifier |= Modifier::DIM.bits(),
        }
        if let Some(bg) = self.color(u32_to_color(cell.bg)) {
            cell.bg = color_to_u32(bg);
        }
    }
}

impl ClientShellState {
    pub(super) fn dim_inactive_pane_cells(
        &self,
        frame: &mut FrameData,
        area: Rect,
        surface: &PaneSurfaceFrame,
    ) {
        if !self.config.dim_inactive_panes || surface.panes.len() < 2 {
            return;
        }
        let dimmer = InactivePaneDimmer::new(&self.config, &self.host_terminal_theme);
        for pane in surface.panes.iter().filter(|pane| !pane.focused) {
            let inner = pane.inner_rect;
            let left = area.x.saturating_add(inner.x);
            let top = area.y.saturating_add(inner.y);
            let right = left
                .saturating_add(inner.width)
                .min(area.right())
                .min(frame.width);
            let bottom = top
                .saturating_add(inner.height)
                .min(area.bottom())
                .min(frame.height);
            for y in top..bottom {
                let start = usize::from(y) * usize::from(frame.width) + usize::from(left);
                let end = usize::from(y) * usize::from(frame.width) + usize::from(right);
                if let Some(cells) = frame.cells.get_mut(start..end) {
                    for cell in cells {
                        dimmer.cell(cell);
                    }
                }
            }
        }
    }
}

fn mix_rgb(base: Rgb, target: Rgb, amount: f32) -> Rgb {
    fn channel(base: u8, target: u8, amount: f32) -> u8 {
        (f32::from(base) + (f32::from(target) - f32::from(base)) * amount).round() as u8
    }
    (
        channel(base.0, target.0, amount),
        channel(base.1, target.1, amount),
        channel(base.2, target.2, amount),
    )
}

fn relative_luminance(color: Rgb) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

fn color_to_rgb(color: Color) -> Option<Rgb> {
    match color {
        Color::Reset => None,
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(_) => None,
    }
}

/// Resolve the color that dimmed inactive-pane cells fade toward: the host
/// terminal's detected background, falling back to theme surfaces.
pub(crate) fn inactive_dim_target(
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) -> Option<(u8, u8, u8)> {
    host_theme
        .background
        .map(terminal_theme_to_rgb)
        .or_else(|| color_to_rgb(p.panel_bg))
        .or_else(|| color_to_rgb(p.surface_dim))
}

/// Standard xterm 256-color palette values. Entries 0-15 use the same
/// approximations as `color_to_rgb`; the host terminal may theme them
/// differently, but a close match is enough for dimming.
fn indexed_to_rgb(index: u8) -> Rgb {
    match index {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let value = |component: u8| {
                if component == 0 {
                    0
                } else {
                    55 + 40 * component
                }
            };
            let offset = index - 16;
            (
                value(offset / 36),
                value((offset % 36) / 6),
                value(offset % 6),
            )
        }
        232..=255 => {
            let gray = 8 + 10 * (index - 232);
            (gray, gray, gray)
        }
    }
}

fn named_ansi_index(color: Color) -> Option<u8> {
    match color {
        Color::Black => Some(0),
        Color::Red => Some(1),
        Color::Green => Some(2),
        Color::Yellow => Some(3),
        Color::Blue => Some(4),
        Color::Magenta => Some(5),
        Color::Cyan => Some(6),
        Color::Gray => Some(7),
        Color::DarkGray => Some(8),
        Color::LightRed => Some(9),
        Color::LightGreen => Some(10),
        Color::LightYellow => Some(11),
        Color::LightBlue => Some(12),
        Color::LightMagenta => Some(13),
        Color::LightCyan => Some(14),
        Color::White => Some(15),
        _ => None,
    }
}

/// Resolve a cell color to RGB for dimming, preferring the host terminal's
/// reported palette (OSC 4) so dimmed colors keep the on-screen hue, and
/// falling back to standard xterm values when the host palette is unknown.
fn dim_color_to_rgb(
    color: Color,
    host_theme: &crate::terminal_theme::TerminalTheme,
) -> Option<Rgb> {
    let host_palette =
        |index: u8| host_theme.palette[usize::from(index)].map(terminal_theme_to_rgb);
    match color {
        Color::Indexed(index) => Some(host_palette(index).unwrap_or_else(|| indexed_to_rgb(index))),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Reset => None,
        named => match named_ansi_index(named) {
            Some(index) => host_palette(index).or_else(|| color_to_rgb(named)),
            None => color_to_rgb(named),
        },
    }
}

#[cfg(test)]
fn dim_color_toward(
    color: Color,
    target: Rgb,
    theme: &TerminalTheme,
    strength: f32,
) -> Option<Color> {
    InactivePaneDimmer::for_target(Some(target), theme, strength).color(color)
}

#[cfg(test)]
fn dimmed_inactive_style(
    style: Style,
    target: Option<Rgb>,
    theme: &TerminalTheme,
    strength: f32,
) -> Style {
    let dimmer = InactivePaneDimmer::for_target(target, theme, strength);
    let mut cell = CellData {
        symbol: String::new(),
        fg: color_to_u32(style.fg.unwrap_or(Color::Reset)),
        bg: color_to_u32(style.bg.unwrap_or(Color::Reset)),
        modifier: style.add_modifier.bits(),
        skip: false,
        hyperlink: None,
    };
    dimmer.cell(&mut cell);
    Style {
        fg: Some(u32_to_color(cell.fg)),
        bg: Some(u32_to_color(cell.bg)),
        add_modifier: Modifier::from_bits_truncate(cell.modifier),
        ..style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dimmed_inactive_style_blends_fg_and_bg_toward_target() {
        let style = Style::default()
            .fg(Color::Rgb(200, 100, 50))
            .bg(Color::Rgb(100, 200, 250));

        let dimmed = dimmed_inactive_style(
            style,
            Some((0, 0, 0)),
            &crate::terminal_theme::TerminalTheme::default(),
            0.4,
        );

        assert_eq!(dimmed.fg, Some(Color::Rgb(120, 60, 30)));
        assert_eq!(dimmed.bg, Some(Color::Rgb(60, 120, 150)));
        assert!(!dimmed.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn dimmed_inactive_style_falls_back_to_dim_for_unblendable_fg() {
        let style = Style::default().fg(Color::Reset).bg(Color::Reset);

        let dimmed = dimmed_inactive_style(
            style,
            Some((0, 0, 0)),
            &crate::terminal_theme::TerminalTheme::default(),
            0.4,
        );

        assert_eq!(dimmed.fg, Some(Color::Reset));
        assert_eq!(dimmed.bg, Some(Color::Reset));
        assert!(dimmed.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn dimmed_inactive_style_resolves_indexed_palette_colors() {
        // Indexed(42) = color-cube entry (0, 215, 135); zsh %K{} prompt
        // segments arrive as indexed colors and must dim with the pane.
        let style = Style::default()
            .fg(Color::Indexed(15))
            .bg(Color::Indexed(42));

        let dimmed = dimmed_inactive_style(
            style,
            Some((0, 0, 0)),
            &crate::terminal_theme::TerminalTheme::default(),
            0.4,
        );

        assert_eq!(dimmed.fg, Some(Color::Rgb(153, 153, 153)));
        assert_eq!(dimmed.bg, Some(Color::Rgb(0, 129, 81)));
        assert!(!dimmed.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn dimmed_inactive_style_prefers_host_palette_over_standard_values() {
        // The host terminal reports its real palette via OSC 4; dimming must
        // start from the on-screen hue (soft blue), not the xterm default
        // navy, or dimmed cells shift to a completely different color.
        let host_theme = crate::terminal_theme::TerminalTheme::default().with_palette_color(
            4,
            crate::terminal_theme::RgbColor {
                r: 108,
                g: 129,
                b: 251,
            },
        );
        let style = Style::default().fg(Color::Blue).bg(Color::Indexed(4));

        let dimmed = dimmed_inactive_style(style, Some((0, 0, 0)), &host_theme, 0.4);

        assert_eq!(dimmed.fg, Some(Color::Rgb(65, 77, 151)));
        assert_eq!(dimmed.bg, Some(Color::Rgb(65, 77, 151)));
    }

    #[test]
    fn dimmed_inactive_style_scales_default_fg_via_host_foreground() {
        let host_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 200,
                g: 200,
                b: 200,
            }),
            ..Default::default()
        };
        let style = Style::default().fg(Color::Reset).bg(Color::Reset);

        let dimmed = dimmed_inactive_style(style, Some((0, 0, 0)), &host_theme, 0.4);

        assert_eq!(dimmed.fg, Some(Color::Rgb(120, 120, 120)));
        assert_eq!(dimmed.bg, Some(Color::Reset));
        assert!(!dimmed.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn dim_color_toward_lightens_on_light_backgrounds() {
        let dimmed = dim_color_toward(
            Color::Rgb(100, 50, 25),
            (250, 250, 250),
            &crate::terminal_theme::TerminalTheme::default(),
            0.4,
        );
        assert_eq!(dimmed, Some(Color::Rgb(162, 132, 117)));
    }

    #[test]
    fn dim_color_toward_strength_controls_the_scale() {
        let theme = crate::terminal_theme::TerminalTheme::default();
        let color = Color::Rgb(200, 100, 50);

        assert_eq!(
            dim_color_toward(color, (0, 0, 0), &theme, 0.0),
            Some(Color::Rgb(200, 100, 50))
        );
        assert_eq!(
            dim_color_toward(color, (0, 0, 0), &theme, 1.0),
            Some(Color::Rgb(0, 0, 0))
        );
    }

    #[test]
    fn dimmed_inactive_style_uses_dim_modifier_without_target() {
        let style = Style::default().fg(Color::Rgb(10, 20, 30));

        let dimmed = dimmed_inactive_style(
            style,
            None,
            &crate::terminal_theme::TerminalTheme::default(),
            0.4,
        );

        assert_eq!(dimmed.fg, Some(Color::Rgb(10, 20, 30)));
        assert!(dimmed.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn inactive_dim_target_prefers_host_background_over_palette() {
        let host_theme = crate::terminal_theme::TerminalTheme {
            background: Some(crate::terminal_theme::RgbColor { r: 1, g: 2, b: 3 }),
            ..Default::default()
        };
        assert_eq!(
            inactive_dim_target(&Palette::catppuccin(), host_theme),
            Some((1, 2, 3))
        );

        let no_host = crate::terminal_theme::TerminalTheme::default();
        let palette = Palette::catppuccin();
        assert_eq!(
            inactive_dim_target(&palette, no_host),
            color_to_rgb(palette.panel_bg)
        );
    }
}
