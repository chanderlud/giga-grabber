use iced::widget::svg::{Status, Style};
use iced::{Color, Theme};

pub(crate) fn svg_icon_style(color: Option<Color>) -> impl Fn(&Theme, Status) -> Style {
    move |_theme: &Theme, _status: Status| Style { color }
}
