use iced::widget::text_input::{Status, Style};
use iced::{Border, Theme, border};

use crate::app::UrlStatus;

pub(crate) struct UrlInput {
    pub(crate) mode: UrlStatus,
}

impl UrlInput {
    pub fn style(&self, theme: &Theme, status: Status) -> Style {
        let palette = theme.extended_palette();
        let border_color = match self.mode {
            UrlStatus::Invalid => palette.danger.strong.color,
            _ => palette.background.strong.color,
        };

        match status {
            Status::Active | Status::Disabled => Style {
                background: palette.background.weak.color.into(),
                border: Border {
                    radius: border::radius(4.0),
                    width: 2.0,
                    color: border_color,
                },
                icon: Default::default(),
                placeholder: palette.background.weak.text.into(),
                value: palette.background.base.text.into(),
                selection: palette.primary.weak.color.into(),
            },
            Status::Focused { .. } | Status::Hovered => Style {
                background: palette.background.base.color.into(),
                border: Border {
                    radius: border::radius(4.0),
                    width: 2.0,
                    color: border_color,
                },
                icon: Default::default(),
                placeholder: palette.background.weak.text.into(),
                value: palette.background.base.text.into(),
                selection: palette.primary.weak.color.into(),
            },
        }
    }
}
