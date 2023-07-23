use iced::widget::scrollable;
use iced::widget::scrollable::{Scrollbar, Scroller};
use iced::{Color, Theme};

pub(crate) struct Scrollable;

impl scrollable::StyleSheet for Scrollable {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Scrollbar {
        Scrollbar {
            background: None,
            border_radius: 0.0,
            border_width: 0.0,
            border_color: Default::default(),
            scroller: Scroller {
                color: Color::from_rgb8(62, 62, 62),
                border_radius: 4.0,
                border_width: 0.0,
                border_color: Default::default(),
            },
        }
    }

    fn hovered(&self, _style: &Self::Style, _is_mouse_over_scrollbar: bool) -> Scrollbar {
        Scrollbar {
            background: None,
            border_radius: 0.0,
            border_width: 0.0,
            border_color: Default::default(),
            scroller: Scroller {
                color: Color::from_rgb8(82, 82, 82),
                border_radius: 4.0,
                border_width: 0.0,
                border_color: Default::default(),
            },
        }
    }
}
