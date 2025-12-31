use iced::widget::button::{Status, Style};
use iced::{Border, Color, Theme, border};

pub(crate) struct Nav {
    pub(crate) active: bool,
}

impl Nav {
    pub fn style(&self, theme: &Theme, status: Status) -> Style {
        let palette = theme.extended_palette();

        match status {
            Status::Active => Style {
                background: Some(if self.active {
                    palette.background.weak.color.into()
                } else {
                    Color::TRANSPARENT.into()
                }),
                border: Border {
                    radius: border::radius(6.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
            Status::Hovered | Status::Pressed => Style {
                background: Some(if self.active {
                    palette.background.weak.color.into()
                } else {
                    palette.background.base.color.into()
                }),
                border: Border {
                    radius: border::radius(6.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
            Status::Disabled => Style {
                background: Some(Color::TRANSPARENT.into()),
                border: Border {
                    radius: border::radius(6.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.weak.text,
                ..Default::default()
            },
        }
    }
}

pub(crate) struct IconButton;

impl IconButton {
    pub fn style(&self, theme: &Theme, status: Status) -> Style {
        let palette = theme.extended_palette();
        match status {
            Status::Active => Style {
                ..Default::default()
            },
            Status::Hovered | Status::Pressed => Style {
                background: Some(palette.background.strong.color.into()),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                ..Default::default()
            },
            Status::Disabled => Style {
                ..Default::default()
            },
        }
    }
}
