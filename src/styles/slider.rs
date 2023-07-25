use iced::{Color, Theme};
use iced_native::widget::slider;
use iced_native::widget::slider::{Appearance, Handle, HandleShape, Rail};

pub(crate) struct Slider;

impl slider::StyleSheet for Slider {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            rail: Rail {
                colors: (Color::from_rgb8(255, 48, 78), Color::from_rgb8(255, 48, 78)),
                width: 8_f32,
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                color: Color::from_rgb8(255, 48, 78),
                border_width: 5_f32,
                border_color: Color::from_rgb8(69, 69, 69),
            },
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            rail: Rail {
                colors: (Color::from_rgb8(255, 68, 98), Color::from_rgb8(255, 68, 98)),
                width: 8_f32,
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                color: Color::from_rgb8(255, 68, 98),
                border_width: 4_f32,
                border_color: Color::from_rgb8(69, 69, 69),
            },
        }
    }

    fn dragging(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            rail: Rail {
                colors: (Color::from_rgb8(235, 28, 58), Color::from_rgb8(235, 28, 58)),
                width: 8_f32,
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                color: Color::from_rgb8(235, 28, 58),
                border_width: 6_f32,
                border_color: Color::from_rgb8(69, 69, 69),
            },
        }
    }
}
