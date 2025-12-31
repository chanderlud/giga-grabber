use iced::widget::container::Style;
use iced::{Border, Theme, border};

pub(crate) struct Download {
    pub(crate) index: usize,
}

impl Download {
    pub(crate) fn style(&self, theme: &Theme) -> Style {
        let palette = theme.extended_palette();
        Style {
            text_color: Some(palette.background.base.text),
            background: Some(if !self.index.is_multiple_of(2) {
                palette.background.base.color.into()
            } else {
                palette.background.strong.color.into()
            }),
            ..Default::default()
        }
    }
}

pub(crate) struct Icon {
    pub(crate) active: bool,
}

impl Icon {
    pub(crate) fn new(active: bool) -> Self {
        Self { active }
    }

    pub(crate) fn style(&self, theme: &Theme) -> Style {
        let palette = theme.extended_palette();
        Style {
            text_color: None,
            background: if self.active {
                Some(palette.background.strong.color.into())
            } else {
                None
            },
            border: Border {
                radius: border::radius(4.0),
                width: 0.0,
                color: Default::default(),
            },
            ..Default::default()
        }
    }
}
