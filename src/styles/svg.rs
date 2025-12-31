use iced::widget::svg::{Status, Style};
use iced::{Color, Theme};

#[derive(Clone)]
pub(crate) struct SvgIcon {
    color: Option<Color>,
}

impl SvgIcon {
    pub(crate) fn new(color: Option<Color>) -> Self {
        Self { color }
    }

    pub fn style(&self, _theme: &Theme, _status: Status) -> Style {
        Style { color: self.color }
    }
}
