use iced::{Color, Theme};
use iced::widget::progress_bar;
use iced::widget::progress_bar::Appearance;

pub(crate) struct ProgressBar;

impl progress_bar::StyleSheet for ProgressBar {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> Appearance {
        Appearance {
            background: Color::from_rgb8(200, 200, 200).into(),
            bar: Color::from_rgb8(53,0,211).into(),
            border_radius: 8.0,
        }
    }
}
