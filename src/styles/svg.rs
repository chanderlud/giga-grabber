use iced::widget::svg;
use iced::widget::svg::Appearance;
use iced::{Color, Theme};

#[derive(Clone)]
pub(crate) struct SvgIcon {
    color: Option<Color>,
}

impl SvgIcon {
    pub(crate) fn new(color: Option<Color>) -> Self {
        Self { color }
    }
}

impl svg::StyleSheet for SvgIcon {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance { color: self.color }
    }
}
