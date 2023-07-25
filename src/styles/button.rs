use iced::widget::button;
use iced::widget::button::Appearance;
use iced::{Color, Theme};

pub(crate) struct Nav {
    pub(crate) active: bool,
}

impl button::StyleSheet for Nav {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: if self.active {
                Color::from_rgb8(70, 70, 86).into()
            } else {
                Color::TRANSPARENT.into()
            },
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255, 255),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: if self.active {
                Color::from_rgb8(70, 70, 86).into()
            } else {
                Color::from_rgb8(40, 40, 56).into()
            },
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255, 255),
        }
    }

    fn disabled(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::TRANSPARENT.into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(180, 180, 180),
        }
    }
}

pub(crate) struct Button;

impl button::StyleSheet for Button {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(255, 48, 78).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255, 255),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(255, 68, 98).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255, 255),
        }
    }
}

pub(crate) struct WarningButton;

impl button::StyleSheet for WarningButton {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(255, 191, 83).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(3, 8, 28),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(255, 201, 103).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(3, 8, 28),
        }
    }
}

pub(crate) struct IconButton;

impl button::StyleSheet for IconButton {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: None,
            border_radius: 0.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Default::default(),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(32, 32, 32).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Default::default(),
        }
    }
}
