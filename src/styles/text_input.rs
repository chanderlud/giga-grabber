use crate::helpers::UrlStatus;
use iced::widget::text_input::{Status, Style};
use iced::{Border, Color, Theme, border};

pub(crate) fn url_input_style(mode: UrlStatus) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, status: Status| {
        let palette = theme.extended_palette();
        let is_vanilla = crate::styles::is_vanilla(theme);

        let border_color = match mode {
            UrlStatus::Invalid => palette.danger.strong.color,
            _ => palette.background.strong.color,
        };

        match status {
            Status::Active | Status::Disabled => Style {
                background: (if is_vanilla {
                    palette.background.weaker.color
                } else {
                    palette.background.base.color
                })
                .into(),
                border: Border {
                    radius: border::radius(4.0),
                    width: 2.0,
                    color: if is_vanilla {
                        match mode {
                            UrlStatus::Invalid => palette.danger.strong.color,
                            _ => palette.background.neutral.color,
                        }
                    } else {
                        border_color
                    },
                },
                icon: Default::default(),
                placeholder: if is_vanilla {
                    palette.secondary.base.color
                } else {
                    palette.background.base.text
                },
                value: if is_vanilla {
                    palette.secondary.strong.color
                } else {
                    palette.background.base.text
                },
                selection: palette.primary.weak.color,
            },
            Status::Focused { .. } | Status::Hovered => Style {
                background: (if is_vanilla {
                    // Legacy behavior: focused/hovered darkens vs active
                    Color::from_rgb8(20, 20, 36)
                } else {
                    palette.background.strong.color
                })
                .into(),
                border: Border {
                    radius: border::radius(4.0),
                    width: 2.0,
                    color: (if is_vanilla {
                        match mode {
                            UrlStatus::Invalid => palette.danger.strong.color,
                            _ => palette.background.weakest.color,
                        }
                    } else {
                        border_color
                    }),
                },
                icon: Default::default(),
                placeholder: if is_vanilla {
                    palette.secondary.base.color
                } else {
                    palette.background.base.text
                },
                value: if is_vanilla {
                    palette.secondary.strong.color
                } else {
                    palette.background.base.text
                },
                selection: palette.primary.weak.color,
            },
        }
    }
}
