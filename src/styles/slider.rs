use iced::Border;
use iced::Theme;
use iced::widget::slider::{Handle, HandleShape, Rail, Status, Style};

pub(crate) fn slider_style(theme: &Theme, status: Status) -> Style {
    let palette = theme.extended_palette();
    let primary_color = palette.primary.strong.color;

    match status {
        Status::Active | Status::Dragged => Style {
            rail: Rail {
                backgrounds: (primary_color.into(), primary_color.into()),
                width: 8_f32,
                border: Border::default().rounded(4.0),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                background: primary_color.into(),
                border_width: 5_f32,
                border_color: palette.background.base.color,
            },
        },
        Status::Hovered => Style {
            rail: Rail {
                backgrounds: (
                    palette.primary.base.color.into(),
                    palette.primary.base.color.into(),
                ),
                width: 8_f32,
                border: Border::default().rounded(4.0),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                background: palette.primary.base.color.into(),
                border_width: 4_f32,
                border_color: palette.background.base.color,
            },
        },
    }
}
