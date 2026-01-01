use iced::widget::container::Style;
use iced::{Border, Theme, border};

pub(crate) fn download_style(index: usize) -> impl Fn(&Theme) -> Style {
    move |theme: &Theme| {
        let palette = theme.extended_palette();
        Style {
            text_color: Some(palette.background.base.text),
            background: Some(if !index.is_multiple_of(2) {
                palette.background.base.color.into()
            } else {
                palette.background.strong.color.into()
            }),
            ..Default::default()
        }
    }
}

pub(crate) fn icon_style(active: bool) -> impl Fn(&Theme) -> Style {
    move |theme: &Theme| {
        let palette = theme.extended_palette();
        Style {
            text_color: None,
            background: if active {
                Some(palette.background.weakest.color.into())
            } else {
                None
            },
            border: Border {
                radius: border::radius(4.0),
                width: 0.0,
                color: Default::default(),
            },
            ..Default::default()
        }
    }
}
