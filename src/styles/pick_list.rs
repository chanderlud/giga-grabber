use iced::widget::pick_list::{Status, Style};
use iced::{Border, Theme};

pub(crate) fn default(theme: &Theme, status: Status) -> Style {
    let palette = theme.extended_palette();

    let active = Style {
        text_color: palette.background.weak.text,
        background: palette.background.weak.color.into(),
        placeholder_color: palette.secondary.base.color,
        handle_color: palette.primary.base.color,
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: palette.background.strong.color,
        },
    };

    match status {
        Status::Active => active,
        Status::Hovered | Status::Opened { .. } => Style {
            border: Border {
                color: palette.primary.strong.color,
                ..active.border
            },
            ..active
        },
    }
}
