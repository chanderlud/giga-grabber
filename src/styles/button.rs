use iced::widget::button;
use iced::widget::button::{Status, Style};
use iced::{Border, Color, Theme, border};

pub(crate) fn navigation(active: bool) -> impl Fn(&Theme, Status) -> Style {
    move |theme: &Theme, status: Status| {
        let palette = theme.extended_palette();
        let is_vanilla = crate::styles::is_vanilla(theme);

        match status {
            Status::Active => Style {
                background: Some(if active {
                    palette.background.strongest.color.into()
                } else {
                    Color::TRANSPARENT.into()
                }),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
            Status::Hovered | Status::Pressed => Style {
                background: Some(if active {
                    palette.background.strongest.color.into()
                } else {
                    (if is_vanilla {
                        palette.background.strong.color
                    } else {
                        palette.background.stronger.color
                    })
                    .into()
                }),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
            Status::Disabled => Style {
                background: Some(Color::TRANSPARENT.into()),
                border: Border {
                    radius: border::radius(4.0),
                    width: 0.0,
                    color: Default::default(),
                },
                text_color: palette.background.base.text,
                ..Default::default()
            },
        }
    }
}

pub(crate) fn primary(theme: &Theme, status: Status) -> Style {
    let mut base = button::primary(theme, status);
    base.border.radius = 4.into();
    base
}

pub(crate) fn warning(theme: &Theme, status: Status) -> Style {
    let mut base = button::warning(theme, status);
    base.border.radius = 4.into();
    base
}

pub(crate) fn icon(theme: &Theme, status: Status) -> Style {
    let mut base = button::background(theme, status);
    base.border.radius = 4.into();

    if status != Status::Hovered {
        base.background = None;
    }

    base
}
