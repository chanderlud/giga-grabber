use iced::widget::progress_bar;
use iced::{Border, Color, Theme};

/// Custom progress bar style that ensures visibility on both odd and even row backgrounds
pub(crate) fn custom_style(theme: &Theme) -> progress_bar::Style {
    let palette = theme.extended_palette();
    let is_vanilla = crate::styles::is_vanilla(theme);
    progress_bar::Style {
        background: (if is_vanilla {
            Color::from_rgb8(200, 200, 200)
        } else {
            palette.background.weak.color
        })
        .into(),
        bar: palette.primary.strong.color.into(),
        border: Border::default().rounded(if is_vanilla { 8 } else { 4 }),
    }
}
