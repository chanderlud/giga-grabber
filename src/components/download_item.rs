use crate::Download;
use crate::app::MONOSPACE;
use crate::helpers::{icon_button, pad_f32};
use crate::resources::{PAUSE_ICON, PLAY_ICON, X_ICON};
use crate::styles;
use iced::alignment::Vertical;
use iced::widget::{Row, container, progress_bar, space, text};
use iced::{Alignment, Element, Length, Theme};

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Pause(String),
    Resume(String),
    Cancel(String),
}

pub(crate) fn download_item<'a>(
    download: &'a Download,
    index: usize,
    theme: &Theme,
) -> Element<'a, Message> {
    let palette = theme.extended_palette();
    let icon_color = Some(palette.primary.base.color);

    let mut progress = download.progress();
    if progress < 0.1 && progress > 0_f32 {
        progress = 0.1;
    }

    let id = download.node.handle.clone();

    let icon_style_pause = styles::svg::svg_icon_style(icon_color);
    let pause_button = if download.is_paused() {
        icon_button(PLAY_ICON, Message::Resume(id.clone()), icon_style_pause)
    } else {
        icon_button(PAUSE_ICON, Message::Pause(id.clone()), icon_style_pause)
    };

    let icon_style_x = styles::svg::svg_icon_style(icon_color);

    container(
        Row::new()
            .height(Length::Fixed(35_f32))
            .width(Length::Fill)
            .align_y(Alignment::Center)
            .push(space::horizontal().width(Length::Fixed(7_f32)))
            .push(
                text(&download.node.name)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .align_y(Vertical::Center),
            )
            .push(space::horizontal().width(Length::Fixed(3_f32)))
            .push(
                progress_bar(0_f32..=1_f32, progress)
                    .style(styles::progress_bar::custom_style)
                    .length(Length::Fixed(80_f32))
                    .girth(Length::Fixed(15_f32)),
            )
            .push(space::horizontal().width(Length::Fixed(10_f32)))
            .push(
                text(format!("{} MB/s", pad_f32(download.speed())).replace('0', "O"))
                    .width(Length::Shrink)
                    .height(Length::Fill)
                    .align_y(Vertical::Center)
                    .font(MONOSPACE)
                    .size(16),
            )
            .push(space::horizontal().width(Length::Fixed(5_f32)))
            .push(icon_button(
                X_ICON,
                Message::Cancel(id.clone()),
                icon_style_x,
            ))
            .push(pause_button)
            .push(space::horizontal().width(Length::Fixed(7_f32))),
    )
    .style(styles::container::download_style(index))
    .into()
}
