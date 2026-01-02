use iced::Theme;
use iced::widget::svg::{Status, Style};

pub(crate) fn nav_svg(disabled: bool) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, _status: Status| {
        let palette = theme.extended_palette();
        let color = if disabled {
            Some(palette.secondary.weak.color)
        } else {
            Some(palette.primary.strong.color)
        };

        Style { color }
    }
}

pub(crate) fn primary_svg(theme: &Theme, _status: Status) -> Style {
    let palette = theme.extended_palette();
    Style {
        color: Some(palette.primary.base.color),
    }
}

pub(crate) fn danger_svg(theme: &Theme, _status: Status) -> Style {
    let palette = theme.extended_palette();
    Style {
        color: Some(palette.danger.strong.color),
    }
}
