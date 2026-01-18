use iced::widget::pick_list::{Status, Style};
use iced::{Border, Theme};

pub(crate) fn default(theme: &Theme, status: Status) -> Style {
    let palette = theme.extended_palette();
    let is_vanilla = crate::styles::is_vanilla(theme);

    let active = Style {
        text_color: palette.background.weak.text,
        background: palette.background.weak.color.into(),
        placeholder_color: palette.secondary.base.color,
        handle_color: if is_vanilla {
            palette.primary.strong.color
        } else {
            palette.primary.base.color
        },
        border: Border {
            radius: 4.0.into(),
            width: 1.0,
            color: if is_vanilla {
                palette.background.weaker.color
            } else {
                palette.background.strong.color
            },
        },
    };

    match status {
        Status::Active => active,
        Status::Hovered | Status::Opened { .. } => Style {
            background: (if is_vanilla {
                palette.background.strongest.color
            } else {
                palette.background.weak.color
            })
            .into(),
            handle_color: palette.primary.base.color,
            border: Border {
                color: if is_vanilla {
                    active.border.color
                } else {
                    palette.primary.strong.color
                },
                ..active.border
            },
            ..active
        },
    }
}
