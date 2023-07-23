use iced::widget::text_input;
use iced::widget::text_input::Appearance;
use iced::{Color, Theme};

use crate::app::UrlStatus;

pub(crate) struct UrlInput {
    pub(crate) mode: UrlStatus,
}

impl text_input::StyleSheet for UrlInput {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            background: Color::from_rgb8(41, 41, 41).into(),
            border_radius: 4.0,
            border_width: 2.0,
            border_color: match self.mode {
                UrlStatus::Invalid => Color::from_rgb8(255, 69, 0),
                _ => Color::from_rgb8(46, 46, 46),
            },
            icon_color: Default::default(),
        }
    }

    fn focused(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            background: Color::from_rgb8(34, 34, 34).into(),
            border_radius: 4.0,
            border_width: 2.0,
            border_color: match self.mode {
                UrlStatus::Invalid => Color::from_rgb8(255, 69, 0),
                _ => Color::from_rgb8(42, 42, 42),
            },
            icon_color: Default::default(),
        }
    }

    fn placeholder_color(&self, _style: &Self::Style) -> Color {
        Color::from_rgb8(210, 210, 210)
    }

    fn value_color(&self, _style: &Self::Style) -> Color {
        Color::from_rgb8(227, 227, 227)
    }

    fn disabled_color(&self, _style: &Self::Style) -> Color {
        Color::from_rgb8(117, 117, 117)
    }

    fn selection_color(&self, _style: &Self::Style) -> Color {
        Color::from_rgb8(0, 120, 212)
    }

    fn disabled(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            background: Color::from_rgb8(38, 38, 38).into(),
            border_radius: 4.0,
            border_width: 2.0,
            border_color: match self.mode {
                UrlStatus::Invalid => Color::from_rgb8(255, 69, 0),
                _ => Color::from_rgb8(42, 42, 42),
            },
            icon_color: Default::default(),
        }
    }
}
