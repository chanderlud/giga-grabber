use iced::widget::container::Style;
use iced::{Border, Color, Theme, border};

pub(crate) fn download_style(index: usize) -> impl Fn(&Theme) -> Style {
    move |theme: &Theme| {
        let palette = theme.extended_palette();
        let is_vanilla = crate::styles::is_vanilla(theme);
        Style {
            text_color: Some(palette.background.base.text),
            background: Some(if !index.is_multiple_of(2) {
                (if is_vanilla {
                    palette.background.weak.color
                } else {
                    palette.background.base.color
                })
                .into()
            } else {
                (if is_vanilla {
                    palette.background.base.color
                } else {
                    palette.background.strong.color
                })
                .into()
            }),
            ..Default::default()
        }
    }
}

pub(crate) fn download_list_style() -> impl Fn(&Theme) -> Style {
    move |theme: &Theme| {
        let palette = theme.extended_palette();
        let is_vanilla = crate::styles::is_vanilla(theme);

        Style {
            text_color: Some(palette.background.base.text),
            background: Some(
                (if is_vanilla {
                    Color::from_rgb8(3, 8, 28)
                } else {
                    palette.background.base.color
                })
                .into(),
            ),
            border: Border {
                radius: border::radius(4.0),
                width: 2.0,
                color: if is_vanilla {
                    Color::from_rgb8(46, 46, 46)
                } else {
                    palette.background.strong.color
                },
            },
            ..Default::default()
        }
    }
}

pub(crate) fn icon_style(active: bool) -> impl Fn(&Theme) -> Style {
    move |theme: &Theme| {
        let palette = theme.extended_palette();
        let is_vanilla = crate::styles::is_vanilla(theme);
        Style {
            text_color: None,
            background: if active {
                Some(
                    (if is_vanilla {
                        palette.background.base.color
                    } else {
                        palette.background.weakest.color
                    })
                    .into(),
                )
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
