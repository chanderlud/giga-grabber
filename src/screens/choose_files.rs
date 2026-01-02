use crate::app::MONOSPACE;
use crate::mega_client::NodeKind;
use crate::resources::{COLLAPSE_ICON, EXPAND_ICON};
use crate::{Download, MegaFile, styles};
use iced::alignment::Vertical;
use iced::widget::*;
use iced::{Element, Length, Theme};
use std::collections::{HashMap, HashSet};

pub(crate) struct ChooseFiles {
    files: Vec<MegaFile>,
    file_filter: HashMap<String, bool>,
    expanded_files: HashMap<String, bool>,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// toggle file/folder selection
    ToggleFile(Box<(bool, MegaFile)>),
    /// expand/collapse folder in tree
    ToggleExpanded(String),
    /// queue selected files for download
    AddFiles,
    /// remove all loaded files
    ClearFiles,
}

pub(crate) enum Action {
    None,
    QueueDownloads(Vec<Download>),
    /// notify parent to clear file handles tracking
    ClearFiles,
}

impl ChooseFiles {
    pub(crate) fn new(files: Vec<MegaFile>) -> Self {
        Self {
            files,
            file_filter: HashMap::new(),
            expanded_files: HashMap::new(),
        }
    }

    pub(crate) fn add_files(&mut self, files: Vec<MegaFile>) {
        self.files.extend(files);
    }

    pub(crate) fn update(&mut self, message: Message, active_handles: &HashSet<String>) -> Action {
        match message {
            Message::ToggleFile(item) => {
                // insert an entry for the file in the filter
                self.file_filter.insert(item.1.node.handle.clone(), item.0);

                // all children of the file should have the same entry in the filter
                item.1.iter().for_each(|file| {
                    self.file_filter.insert(file.node.handle.clone(), item.0);
                });

                Action::None
            }
            Message::ToggleExpanded(hash) => {
                if let Some(expanded) = self.expanded_files.get_mut(&hash) {
                    // toggle expanded state if it already exists
                    *expanded = !*expanded;
                } else {
                    // insert expanded state if it doesn't exist
                    self.expanded_files.insert(hash, true);
                }

                Action::None
            }
            Message::AddFiles => {
                // flatten file structure into a list of downloads
                let downloads: Vec<Download> = self
                    .files
                    .iter()
                    .flat_map(|file| file.iter())
                    .filter(|f| f.node.kind == NodeKind::File)
                    .filter(|f| *self.file_filter.get(&f.node.handle).unwrap_or(&true))
                    .filter(|f| !active_handles.contains(&f.node.handle))
                    .map(Download::new)
                    .collect();

                Action::QueueDownloads(downloads)
            }
            Message::ClearFiles => Action::ClearFiles,
        }
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut column = Column::new().width(Length::Fill).spacing(5);

        let size: u64 = self
            .files
            .iter()
            .flat_map(|file| file.iter())
            .filter(|f| f.node.kind == NodeKind::File)
            .filter(|f| *self.file_filter.get(&f.node.handle).unwrap_or(&true))
            .map(|file| file.node.size)
            .sum();
        let size_gb = size as f64 / 1024f64.powi(3);

        for file in &self.files {
            column = column.push(self.recursive_files(file));
        }

        container(
            Column::new()
                .push(scrollable(column).width(Length::Fill).height(Length::Fill))
                .push(
                    Row::new()
                        .height(Length::Fixed(30_f32))
                        .spacing(10)
                        .push(
                            button(" Add to queue ")
                                .style(styles::button::primary)
                                .on_press(Message::AddFiles),
                        )
                        .push(
                            button(" Cancel ")
                                .style(styles::button::warning)
                                .on_press(Message::ClearFiles),
                        )
                        .push(
                            container(
                                text(format!(" {:.2} GB ", size_gb).replace('0', "O"))
                                    .font(MONOSPACE)
                                    .align_y(Vertical::Center)
                                    .align_x(iced::alignment::Horizontal::Center)
                                    .width(Length::Fill)
                                    .height(Length::Fill),
                            )
                            .style(|theme: &Theme| {
                                let palette = theme.extended_palette();
                                container::Style {
                                    background: Some(palette.background.strong.color.into()),
                                    border: iced::Border::default().rounded(4.0),
                                    ..Default::default()
                                }
                            })
                            .height(Length::Fill),
                        ),
                ),
        )
        .into()
    }

    fn recursive_files<'a>(&'a self, file: &'a MegaFile) -> Element<'a, Message> {
        if file.children.is_empty() {
            Row::new()
                .spacing(5)
                .push(
                    text(&file.node.name)
                        .width(Length::Fill)
                        .align_y(Vertical::Center),
                )
                .push(
                    checkbox(*self.file_filter.get(&file.node.handle).unwrap_or(&true))
                        .on_toggle(|value| Message::ToggleFile(Box::new((value, file.clone()))))
                        .style(checkbox::primary),
                )
                .into()
        } else {
            let expanded = *self.expanded_files.get(&file.node.handle).unwrap_or(&false);

            let mut column = Column::new().spacing(5).push(
                Row::new()
                    .spacing(5)
                    .push(
                        button(
                            svg(svg::Handle::from_memory(if expanded {
                                COLLAPSE_ICON
                            } else {
                                EXPAND_ICON
                            }))
                            .height(Length::Fixed(16_f32))
                            .width(Length::Fixed(16_f32))
                            .style(styles::svg::primary_svg),
                        )
                        .style(styles::button::icon)
                        .on_press(Message::ToggleExpanded(file.node.handle.clone()))
                        .padding(3),
                    )
                    .push(
                        text(&file.node.name)
                            .width(Length::Fill)
                            .align_y(Vertical::Center),
                    )
                    .push(
                        checkbox(*self.file_filter.get(&file.node.handle).unwrap_or(&true))
                            .on_toggle(|value| Message::ToggleFile(Box::new((value, file.clone()))))
                            .style(checkbox::primary),
                    ),
            );

            if expanded {
                for file in &file.children {
                    column = column.push(
                        Row::new()
                            .push(space::horizontal().width(Length::Fixed(20.0)))
                            .push(self.recursive_files(file)),
                    );
                }
            }

            column.into()
        }
    }
}
