use iced::{Color, Theme};
use iced::widget::button;
use iced::widget::button::Appearance;

pub(crate) struct Nav {
    pub(crate) active: bool
}

impl button::StyleSheet for Nav {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: if self.active {
                Color::from_rgb8(76, 76 , 76).into()
            } else {
                Color::TRANSPARENT.into()
            },
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255 , 255).into(),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: if self.active {
                Color::from_rgb8(66, 66 , 66).into()
            } else {
                Color::from_rgb8(76, 76 , 76).into()
            },
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255 , 255).into(),
        }
    }

    fn disabled(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::TRANSPARENT.into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(180, 180 , 180).into(),
        }
    }
}

pub(crate) struct Button;

impl button::StyleSheet for Button {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(53,0,211).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255 , 255).into(),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(73,0,231).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255 , 255).into(),
        }
    }
}

pub(crate) struct WarningButton;

impl button::StyleSheet for WarningButton {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(205, 69, 0).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255 , 255).into(),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            shadow_offset: Default::default(),
            background: Color::from_rgb8(255, 69, 0).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Default::default(),
            text_color: Color::from_rgb8(255, 255 , 255).into(),
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
