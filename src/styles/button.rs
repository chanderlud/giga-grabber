use iced::widget::button::{Status, Style};
use iced::{Border, Color, Theme, border};

pub(crate) struct Nav {
    pub(crate) active: bool,
}

impl Nav {
    pub(crate) fn style(&self, theme: &Theme, status: Status) -> Style {
        let palette = theme.extended_palette();

        match status {
            Status::Active => Style {
                background: Some(if self.active {
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
                background: Some(if self.active {
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
