use iced::widget::checkbox;
use iced::widget::checkbox::Appearance;
use iced::{Color, Theme};

pub(crate) struct Checkbox;

impl checkbox::StyleSheet for Checkbox {
    type Style = Theme;

    fn active(&self, _style: &Self::Style, is_checked: bool) -> Appearance {
        Appearance {
            background: if is_checked {
                Color::from_rgb8(53, 0, 211).into()
            } else {
                Color::from_rgb8(73, 0, 231).into()
            },
            icon_color: Color::from_rgb8(255, 255, 255),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: None,
        }
    }

    fn hovered(&self, _style: &Self::Style, is_checked: bool) -> Appearance {
        Appearance {
            background: if is_checked {
                Color::from_rgb8(73, 0, 231).into()
            } else {
                Color::from_rgb8(53, 0, 211).into()
            },
            icon_color: Color::from_rgb8(220, 220, 220),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: None,
        }
    }
}
