use crate::app::MONOSPACE;
use crate::app::helpers::{icon_button, pad_f32};
use crate::app::screens::{PAUSE_ICON, PLAY_ICON, X_ICON};
use crate::app::styles;
use crate::{Download, format_size};
use iced::alignment::Vertical;
use iced::widget::{Row, container, progress_bar, space, text};
use iced::{Alignment, Element, Length};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Pause(String),
    Resume(String),
    Cancel(String),
}

#[derive(Clone, Copy)]
pub(crate) struct DisplayPreferences {
    pub(crate) show_progress_bars: bool,
    pub(crate) show_download_sizes: bool,
}

const PROGRESS_COLUMN_WIDTH: f32 = 80.0;
const SIZE_COLUMN_WIDTH: f32 = 105.0;
const SPEED_COLUMN_WIDTH: f32 = 90.0;

pub(crate) fn download_item(
    download: &'_ Download,
    index: usize,
    preferences: DisplayPreferences,
) -> Element<'_, Message> {
    let mut progress = download.progress();
    if progress < 0.1 && progress > 0_f32 {
        progress = 0.1;
    }

    let id = download.node.handle.clone();

    let pause_button = if download.is_paused() {
        icon_button(
            PLAY_ICON,
            Message::Resume(id.clone()),
            styles::svg::primary_svg,
        )
    } else {
        icon_button(
            PAUSE_ICON,
            Message::Pause(id.clone()),
            styles::svg::primary_svg,
        )
    };

    let mut row = Row::new()
        .height(Length::Fixed(35_f32))
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .push(space::horizontal().width(Length::Fixed(7_f32)))
        .push(
            text(&download.node.name)
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(Vertical::Center),
        );

    if preferences.show_progress_bars {
        row = row
            .push(space::horizontal().width(Length::Fixed(3_f32)))
            .push(
                progress_bar(0_f32..=1_f32, progress)
                    .style(styles::progress_bar::custom_style)
                    .length(Length::Fixed(PROGRESS_COLUMN_WIDTH))
                    .girth(Length::Fixed(15_f32)),
            )
            .push(space::horizontal().width(Length::Fixed(10_f32)));
    }

    if preferences.show_download_sizes {
        row = row
            .push(
                text({
                    let downloaded = download.downloaded.load(Ordering::Relaxed) as u64;
                    let total = download.node.size;
                    format!("{}/{}", format_size(downloaded), format_size(total)).replace('0', "O")
                })
                .width(Length::Fixed(SIZE_COLUMN_WIDTH))
                .height(Length::Fill)
                .align_y(Vertical::Center)
                .font(MONOSPACE)
                .size(13),
            )
            .push(space::horizontal().width(Length::Fixed(6_f32)));
    }

    row = row
        .push(
            text(format!("{} MB/s", pad_f32(download.speed())).replace('0', "O"))
                .width(Length::Fixed(SPEED_COLUMN_WIDTH))
                .height(Length::Fill)
                .align_y(Vertical::Center)
                .font(MONOSPACE)
                .size(16),
        )
        .push(space::horizontal().width(Length::Fixed(5_f32)))
        .push(icon_button(
            X_ICON,
            Message::Cancel(id.clone()),
            styles::svg::primary_svg,
        ))
        .push(pause_button)
        .push(space::horizontal().width(Length::Fixed(7_f32)));

    container(row)
        .style(styles::container::download_style(index))
        .into()
}
