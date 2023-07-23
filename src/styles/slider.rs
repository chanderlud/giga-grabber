use iced::{Color, Theme};
use iced_native::widget::slider;
use iced_native::widget::slider::{Appearance, Handle, HandleShape, Rail};

pub(crate) struct Slider;

impl slider::StyleSheet for Slider {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            rail: Rail {
                colors: (Color::from_rgb8(96, 205, 255), Color::from_rgb8(96, 205, 255)),
                width: 8.0
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 12.0 },
                color: Color::from_rgb8(96, 205, 255),
                border_width: 5.0,
                border_color: Color::from_rgb8(69, 69, 69)
            }
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            rail: Rail {
                colors: (Color::from_rgb8(90, 188, 232), Color::from_rgb8(90, 188, 232)),
                width: 8.0
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 12.0 },
                color: Color::from_rgb8(94, 192, 236),
                border_width: 4.0,
                border_color: Color::from_rgb8(69, 69, 69)
            }
        }
    }

    fn dragging(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            rail: Rail {
                colors: (Color::from_rgb8(83, 170, 210), Color::from_rgb8(83, 170, 210)),
                width: 8.0
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 12.0 },
                color: Color::from_rgb8(91, 178, 218),
                border_width: 6.0,
                border_color: Color::from_rgb8(69, 69, 69)
            }
        }
    }
}
