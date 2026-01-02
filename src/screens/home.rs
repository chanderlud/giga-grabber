use crate::app::MONOSPACE;
use crate::components::download_item;
use crate::{Download, styles};
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{Column, Row, button, container, scrollable, text};
use iced::{Border, Element, Length, Theme};
use std::collections::HashMap;
use std::sync::atomic::Ordering::Relaxed;

pub(crate) struct Home {
    active_downloads: HashMap<String, Download>,
    errors: Vec<String>,
    all_paused: bool,
    bandwidth_counter: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    CancelDownloads,
    CancelDownload(String),
    PauseDownloads,
    PauseDownload(String),
    ResumeDownloads,
    ResumeDownload(String),
}

pub(crate) enum Action {
    None,
    StopWorkers,
}

impl Home {
    pub(crate) fn new() -> Self {
        Self {
            active_downloads: HashMap::new(),
            errors: Vec::new(),
            all_paused: false,
            bandwidth_counter: 0,
        }
    }

    pub(crate) fn add_active_download(&mut self, download: Download) {
        self.active_downloads
            .insert(download.node.handle.clone(), download);
    }

    pub(crate) fn remove_active_download(&mut self, id: &str) -> Option<Download> {
        let download = self.active_downloads.remove(id)?;
        self.bandwidth_counter += download.downloaded.load(Relaxed);
        Some(download)
    }

    pub(crate) fn add_error(&mut self, error: String) {
        self.errors.push(error);
    }

    pub(crate) fn active_downloads(&self) -> &HashMap<String, Download> {
        &self.active_downloads
    }

    pub(crate) fn has_active_downloads(&self) -> bool {
        !self.active_downloads.is_empty()
    }

    #[allow(dead_code)]
    pub(crate) fn bandwidth_counter(&self) -> usize {
        self.bandwidth_counter
    }

    pub(crate) fn update(&mut self, message: Message) -> Action {
        match message {
            Message::CancelDownloads => {
                for (_, download) in self.active_downloads.drain() {
                    download.cancel();
                }
                Action::StopWorkers
            }
            Message::CancelDownload(id) => {
                if let Some(download) = self.active_downloads.get(&id) {
                    download.cancel();
                }
                Action::None
            }
            Message::PauseDownloads => {
                self.all_paused = true;
                for (_, download) in self.active_downloads.iter() {
                    download.pause();
                }
                Action::None
            }
            Message::PauseDownload(id) => {
                if let Some(download) = self.active_downloads.get(&id) {
                    download.pause();
                }
                Action::None
            }
            Message::ResumeDownloads => {
                self.all_paused = false;
                for (_, download) in self.active_downloads.iter() {
                    download.resume();
                }
                Action::None
            }
            Message::ResumeDownload(id) => {
                self.all_paused = false; // can't be all paused if resuming one
                if let Some(download) = self.active_downloads.get(&id) {
                    download.resume();
                }
                Action::None
            }
        }
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut download_list = Column::new();

        for (index, (_id, download)) in self.active_downloads.iter().enumerate() {
            download_list = download_list.push(download_item::download_item(download, index).map(
                |msg| match msg {
                    download_item::Message::Pause(id) => Message::PauseDownload(id),
                    download_item::Message::Resume(id) => Message::ResumeDownload(id),
                    download_item::Message::Cancel(id) => Message::CancelDownload(id),
                },
            ));
        }

        if self.active_downloads.is_empty() {
            download_list = download_list.push(
                text("No active downloads")
                    .height(Length::Fixed(30_f32))
                    .width(Length::Fixed(165_f32))
                    .align_y(Vertical::Center)
                    .align_x(Horizontal::Center),
            )
        }

        let mut download_group = Column::new().push(scrollable(download_list).height(Length::Fill));

        if !self.active_downloads.is_empty() {
            let bandwidth_gb = self.bandwidth_counter as f64 / 1024f64.powi(3);
            download_group = download_group.push(
                Row::new()
                    .spacing(10)
                    .padding(8)
                    .height(Length::Fixed(45_f32))
                    .push(if self.all_paused {
                        button(" Resume All ")
                            .on_press(Message::ResumeDownloads)
                            .style(styles::button::primary)
                    } else {
                        button(" Pause All ")
                            .on_press(Message::PauseDownloads)
                            .style(styles::button::primary)
                    })
                    .push(
                        button(" Cancel All ")
                            .on_press(Message::CancelDownloads)
                            .style(styles::button::warning),
                    )
                    .push(
                        container(
                            text(format!(" {bandwidth_gb:.2} GB used ").replace('0', "O"))
                                .font(MONOSPACE)
                                .align_y(Vertical::Center)
                                .height(Length::Fill),
                        )
                        .style(|theme: &Theme| {
                            let palette = theme.extended_palette();
                            container::Style {
                                background: Some(palette.background.strong.color.into()),
                                border: Border::default().rounded(4.0),
                                ..Default::default()
                            }
                        })
                        .height(Length::Fill),
                    ),
            )
        }

        let mut error_log = Column::new().push(scrollable(self.error_log()));

        if self.errors.is_empty() {
            error_log = error_log.push(
                text("No errors")
                    .height(Length::Fixed(30_f32))
                    .width(Length::Fixed(70_f32))
                    .align_y(Vertical::Center)
                    .align_x(Horizontal::Center),
            )
        }

        container(
            Column::new()
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(5)
                .push(
                    container(download_group)
                        .style(container::bordered_box)
                        .padding(2)
                        .width(Length::Fill)
                        .height(Length::FillPortion(2)),
                )
                .push(
                    container(error_log)
                        .style(container::bordered_box)
                        .padding(8)
                        .width(Length::Fill)
                        .height(Length::FillPortion(1)),
                ),
        )
        .into()
    }

    fn error_log(&self) -> Element<'_, Message> {
        let mut column = Column::new().spacing(2).width(Length::Fill);

        for error in &self.errors {
            column = column.push(text(error).style(text::danger));
        }

        column.into()
    }
}
