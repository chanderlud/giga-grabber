use iced::overlay::menu::{Appearance, StyleSheet};
use iced::{Color, Theme};

pub(crate) struct Menu;

impl StyleSheet for Menu {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255),
            background: Color::from_rgb8(70, 70, 86).into(),
            border_width: 1.0,
            border_radius: 4.0,
            border_color: Color::from_rgb8(30, 30, 46),
            selected_text_color: Color::from_rgb8(225, 225, 225),
            selected_background: Color::from_rgb8(90, 90, 106).into(),
        }
    }
}
