use iced::widget::pick_list::{Appearance, StyleSheet};
use iced::{Color, Theme};

pub(crate) struct PickList;

impl StyleSheet for PickList {
    type Style = Theme;

    fn active(&self, _style: &<Self as StyleSheet>::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255),
            placeholder_color: Default::default(),
            handle_color: Color::from_rgb8(255, 48, 78),
            background: Color::from_rgb8(50, 50, 66).into(),
            border_radius: 4.0,
            border_width: 1.0,
            border_color: Color::from_rgb8(30, 30, 46),
        }
    }

    fn hovered(&self, _style: &<Self as StyleSheet>::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255),
            placeholder_color: Default::default(),
            handle_color: Color::from_rgb8(255, 68, 98),
            background: Color::from_rgb8(70, 70, 86).into(),
            border_radius: 4.0,
            border_width: 1.0,
            border_color: Color::from_rgb8(30, 30, 46),
        }
    }
}
