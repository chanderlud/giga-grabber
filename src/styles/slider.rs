use iced::Border;
use iced::Theme;
use iced::widget::slider::{Handle, HandleShape, Rail, Status, Style};

pub(crate) fn slider_style(theme: &Theme, status: Status) -> Style {
    let palette = theme.extended_palette();
    let danger_color = palette.danger.strong.color;

    match status {
        Status::Active | Status::Dragged => Style {
            rail: Rail {
                backgrounds: (danger_color.into(), danger_color.into()),
                width: 8_f32,
                border: Border::default().rounded(4.0),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                background: danger_color.into(),
                border_width: 5_f32,
                border_color: palette.background.weak.color,
            },
        },
        Status::Hovered => Style {
            rail: Rail {
                backgrounds: (
                    palette.danger.base.color.into(),
                    palette.danger.base.color.into(),
                ),
                width: 8_f32,
                border: Border::default().rounded(4.0),
            },
            handle: Handle {
                shape: HandleShape::Circle { radius: 10_f32 },
                background: palette.danger.base.color.into(),
                border_width: 4_f32,
                border_color: palette.background.weak.color,
            },
        },
    }
}
