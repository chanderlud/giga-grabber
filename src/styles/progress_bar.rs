use iced::widget::progress_bar;
use iced::{Border, Theme};

/// Custom progress bar style that ensures visibility on both odd and even row backgrounds
pub(crate) fn custom_style(theme: &Theme) -> progress_bar::Style {
    let palette = theme.extended_palette();
    progress_bar::Style {
        background: palette.background.weak.color.into(),
        bar: palette.primary.strong.color.into(),
        border: Border::default(),
    }
}
