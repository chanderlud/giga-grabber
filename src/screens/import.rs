use crate::MegaFile;
use crate::get_files;
use crate::helpers::{IndexMap, UrlInput, UrlStatus};
use crate::loading_wheel::LoadingWheelWidget;
use crate::mega_client::MegaClient;
use crate::resources::{CHECK_ICON, TRASH_ICON};
use crate::styles;
use iced::widget::*;
use iced::{Alignment, Element, Length, Task, clipboard};
use regex::Regex;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// trigger clipboard read
    AddUrlClipboard,
    /// clipboard contents received
    GotClipboard(Option<String>),
    /// load files from URL at index
    AddUrl(usize),
    /// load all URLs
    AddAllUrls,
    /// file loading result
    GotFiles(Result<(Vec<MegaFile>, usize), usize>),
    /// URL text input changed
    UrlInput((usize, String)),
    /// add new URL input field
    AddInput,
    /// remove URL input field
    RemoveInput(usize),
}

pub(crate) enum Action {
    None,
    Run(Task<Message>),
    FilesLoaded(Vec<MegaFile>),
    ShowError(String),
}

pub(crate) struct Import {
    url_input: IndexMap<UrlInput>,
    url_regex: Regex,
}

impl Import {
    pub(crate) fn new() -> Self {
        Self {
            url_input: IndexMap::default(),
            url_regex: Regex::new(r"https?://mega\.nz/(folder|file)/([\dA-Za-z]+)#([\dA-Za-z-_]+)")
                .unwrap(),
        }
    }

    pub(crate) fn clear_loaded_inputs(&mut self) {
        self.url_input
            .data
            .retain(|_, input| input.status != UrlStatus::Loaded);
    }

    pub(crate) fn update(&mut self, message: Message, mega: &MegaClient) -> Action {
        match message {
            Message::AddUrlClipboard => Action::Run(clipboard::read().map(Message::GotClipboard)),
            Message::GotClipboard(contents) => {
                if let Some(input) = contents {
                    let stripped = input.trim();
                    if self.url_regex.is_match(stripped) {
                        // create new url input with url as value
                        let index = self.url_input.insert(UrlInput {
                            value: stripped.to_string(),
                            status: UrlStatus::None,
                        });

                        // load the url
                        Action::Run(Task::perform(async move { index }, Message::AddUrl))
                    } else {
                        Action::ShowError("Invalid URL".to_string())
                    }
                } else {
                    Action::ShowError("Clipboard is empty".to_string())
                }
            }
            Message::AddUrl(index) => {
                // get input from index
                if let Some(input) = self.url_input.get_mut(index) {
                    // check if url is valid
                    if !self.url_regex.is_match(&input.value) {
                        input.status = UrlStatus::Invalid;
                        Action::None
                    } else {
                        match input.status {
                            // dont do anything if url is already loading or loaded
                            UrlStatus::Loading | UrlStatus::Loaded => Action::None,
                            _ => {
                                input.status = UrlStatus::Loading; // set status to loading
                                let url = input.value.clone();

                                Action::Run(Task::perform(
                                    get_files(mega.clone(), url, index),
                                    Message::GotFiles,
                                ))
                            }
                        }
                    }
                } else {
                    Action::ShowError("An error occurred".to_string())
                }
            }
            Message::AddAllUrls => {
                let commands: Vec<_> = self
                    .url_input
                    .data
                    .keys()
                    .cloned()
                    .map(|index| Task::perform(async move { index }, Message::AddUrl))
                    .collect();

                Action::Run(Task::batch(commands))
            }
            Message::GotFiles(result) => match result {
                Ok((files, index)) => {
                    if let Some(input) = self.url_input.get_mut(index) {
                        input.status = UrlStatus::Loaded;
                        Action::FilesLoaded(files)
                    } else {
                        Action::ShowError("An error occurred".to_string())
                    }
                }
                Err(index) => {
                    if let Some(input) = self.url_input.get_mut(index) {
                        input.status = UrlStatus::Invalid;
                        Action::None
                    } else {
                        Action::ShowError("An error occurred".to_string())
                    }
                }
            },
            Message::UrlInput((index, value)) => {
                if let Some(input) = self.url_input.get_mut(index) {
                    input.value = value;
                } else {
                    // Use IndexMap bookkeeping to avoid stale indices in `unused_indices` / `next_index`.
                    self.url_input.update(
                        index,
                        UrlInput {
                            value,
                            status: UrlStatus::None,
                        },
                    );
                }
                Action::None
            }
            Message::AddInput => {
                self.url_input.insert(UrlInput {
                    value: String::new(),
                    status: UrlStatus::None,
                });
                Action::None
            }
            Message::RemoveInput(index) => {
                self.url_input.remove(index);
                Action::None
            }
        }
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        container(
            Column::new()
                .spacing(5)
                .push(scrollable(self.url_inputs()).height(Length::Fill))
                .push(
                    Row::new()
                        .spacing(10)
                        .push(
                            button(" Add from clipboard ")
                                .style(styles::button::primary)
                                .on_press(Message::AddUrlClipboard),
                        )
                        .push(
                            button(" + ")
                                .style(styles::button::primary)
                                .on_press(Message::AddInput),
                        )
                        .push(
                            button(" Load all ")
                                .style(styles::button::primary)
                                .on_press(Message::AddAllUrls),
                        ),
                ),
        )
        .into()
    }

    fn url_inputs(&self) -> Element<'_, Message> {
        let mut inputs = Column::new().spacing(5);

        for (index, input) in self.url_input.data.iter() {
            let mut text_input = text_input("Url", &input.value)
                .style(styles::text_input::url_input_style(input.status))
                .size(18)
                .padding(8);

            if input.status == UrlStatus::Invalid || input.status == UrlStatus::None {
                text_input = text_input
                    .on_input(|value| Message::UrlInput((*index, value)))
                    .on_submit(Message::AddUrl(*index));
            }

            let mut row = Row::new()
                .spacing(5)
                .align_y(Alignment::Center)
                .push(text_input);

            match input.status {
                UrlStatus::None | UrlStatus::Invalid => {
                    row = row.push(
                        button(
                            svg(svg::Handle::from_memory(TRASH_ICON))
                                .width(Length::Fixed(22_f32))
                                .height(Length::Fixed(22_f32))
                                .style(styles::svg::danger_svg),
                        )
                        .style(styles::button::icon)
                        .on_press(Message::RemoveInput(*index))
                        .padding(4),
                    );
                }
                UrlStatus::Loading => {
                    row = row.push(LoadingWheelWidget::new().size(30.0));
                }
                UrlStatus::Loaded => {
                    row = row.push(
                        container(
                            svg(svg::Handle::from_memory(CHECK_ICON))
                                .width(Length::Fixed(26_f32))
                                .height(Length::Fixed(26_f32))
                                .style(styles::svg::primary_svg),
                        )
                        .padding(2),
                    );
                }
            }

            inputs = inputs.push(row);
        }

        inputs.into()
    }
}
