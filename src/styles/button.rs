use iced::widget::button::{Status, Style};
use iced::{Border, Color, Theme, border};

pub(crate) fn nav_style(active: bool) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, status: Status| {
        let palette = theme.extended_palette();

        match status {
            Status::Active => Style {
                background: Some(if active {
                    palette.background.strong.color.into()
                } else {
                    Color::TRANSPARENT.into()
                }),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
            Status::Hovered | Status::Pressed => Style {
                background: Some(if active {
                    palette.background.strong.color.into()
                } else {
                    palette.background.stronger.color.into()
                }),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
            Status::Disabled => Style {
                background: Some(Color::TRANSPARENT.into()),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
        }
    }
}
