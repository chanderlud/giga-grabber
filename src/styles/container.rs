use iced::widget::container;
use iced::widget::container::Appearance;
use iced::{Color, Theme};

pub(crate) struct Body;

impl container::StyleSheet for Body {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255).into(),
            background: Color::from_rgb8(3, 8, 28).into(),
            border_radius: 0.0,
            border_width: 0.0,
            border_color: Default::default(),
        }
    }
}

pub(crate) struct Nav;

impl container::StyleSheet for Nav {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255).into(),
            background: Color::from_rgb8(50, 50, 66).into(),
            border_radius: 0.0,
            border_width: 0.0,
            border_color: Default::default(),
        }
    }
}

pub(crate) struct DownloadList;

impl container::StyleSheet for DownloadList {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255).into(),
            background: Color::from_rgb8(3, 8, 28).into(),
            border_radius: 4.0,
            border_width: 2.0,
            border_color: Color::from_rgb8(46, 46, 46),
        }
    }
}

pub(crate) struct Download {
    pub(crate) index: usize,
}

impl container::StyleSheet for Download {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255).into(),
            background: if self.index % 2 != 0 {
                Color::from_rgb8(50, 50, 66).into()
            } else {
                Color::from_rgb8(3, 8, 28).into()
            },
            border_radius: 0.0,
            border_width: 0.0,
            border_color: Default::default(),
        }
    }
}

pub(crate) struct Modal;

impl container::StyleSheet for Modal {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: None,
            background: Color::from_rgb8(41, 41, 41).into(),
            border_radius: 4.0,
            border_width: 2.0,
            border_color: Color::from_rgb8(46, 46, 46),
        }
    }
}

pub(crate) struct Icon {
    pub(crate) active: bool,
}

impl Icon {
    pub(crate) fn new(active: bool) -> Self {
        Self { active }
    }
}

impl container::StyleSheet for Icon {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: None,
            background: if self.active {
                Color::from_rgb8(3, 8, 28).into()
            } else {
                Color::TRANSPARENT.into()
            },
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        }
    }
}

pub(crate) struct Pill;

impl container::StyleSheet for Pill {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            text_color: Color::from_rgb8(255, 255, 255).into(),
            background: Color::from_rgb8(52, 52, 52).into(),
            border_radius: 4.0,
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        }
    }
}
