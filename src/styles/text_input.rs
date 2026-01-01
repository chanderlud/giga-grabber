use crate::helpers::UrlStatus;
use iced::widget::text_input::{Status, Style};
use iced::{Border, Theme, border};

pub(crate) fn url_input_style(mode: UrlStatus) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, status: Status| {
        let palette = theme.extended_palette();
        let border_color = match mode {
            UrlStatus::Invalid => palette.danger.strong.color,
            _ => palette.background.strong.color,
        };

        match status {
            Status::Active | Status::Disabled => Style {
                background: palette.background.base.color.into(),
                border: Border {
                    radius: border::radius(4.0),
                    width: 2.0,
                    color: border_color,
                },
                icon: Default::default(),
                placeholder: palette.background.base.text,
                value: palette.background.base.text,
                selection: palette.primary.weak.color,
            },
            Status::Focused { .. } | Status::Hovered => Style {
                background: palette.background.strong.color.into(),
                border: Border {
                    radius: border::radius(4.0),
                    width: 2.0,
                    color: border_color,
                },
                icon: Default::default(),
                placeholder: palette.background.base.text,
                value: palette.background.base.text,
                selection: palette.primary.weak.color,
            },
        }
    }
}
