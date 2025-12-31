use crate::config::Config;
use crate::helpers::*;
use crate::loading_wheel::LoadingWheelWidget;
use crate::mega_client::{MegaClient, NodeKind};
use crate::resources::*;
use crate::styles::svg::SvgIcon;
use crate::{Download, MegaFile, ProxyMode, RunnerMessage, get_files, spawn_workers, styles};
use futures::future::join_all;
use iced::alignment::{Horizontal, Vertical};
use iced::time::every;
use iced::widget::{Column, Row, slider, space, svg};
use iced::widget::{
    button, center, checkbox, container, mouse_area, opaque, pick_list, progress_bar, scrollable,
    stack, text, text_input,
};
use iced::{Alignment, Border, Color, Element, Font, Length, Subscription, Task, Theme, clipboard};
use log::error;
use native_dialog::FileDialog;
use num_traits::cast::ToPrimitive;
use regex::Regex;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Duration;
use iced::font::{Family, Weight};
use tokio::sync::mpsc::Sender as TokioSender;
use tokio_util::sync::CancellationToken;

const MONOSPACE: Font = Font {
    family: Family::Name("Inconsolata"),
    weight: Weight::Medium,
    ..Font::DEFAULT
};

pub(crate) struct App {
    config: Config,
    mega: MegaClient,
    worker: Option<WorkerState>,
    active_downloads: HashMap<String, Download>,
    runner_sender: Option<TokioSender<RunnerMessage>>,
    download_sender: kanal::Sender<Download>,
    download_receiver: kanal::AsyncReceiver<Download>,
    files: Vec<MegaFile>,
    file_filter: HashMap<String, bool>,
    file_handles: HashSet<String>,
    url_input: IndexMap<UrlInput>,
    expanded_files: HashMap<String, bool>,
    route: Route,
    url_regex: Regex,
    proxy_regex: Regex,
    errors: Vec<String>,
    error_modal: Option<String>,
    all_paused: bool,
    bandwidth_counter: usize,
    rebuild_available: bool,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let config = Config::load().expect("failed to load config");
        (config.into(), Task::none())
    }

    fn title(&self) -> String {
        let mut title = String::from("Giga Grabber");

        // runner is None when not in use
        if self.worker.is_some() {
            title.push_str(" - downloads active");
        }

        if !self.active_downloads.is_empty() {
            title.push_str(&format!(" - {} running", self.active_downloads.len()));
        }

        let queued = self.download_receiver.len();
        if queued > 0 {
            title.push_str(&format!(" - {} queued", queued));
        }

        title
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Refresh => Task::none(),
            Message::AddUrlClipboard => clipboard::read().map(Message::GotClipboard),
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
                        Task::perform(async move { index }, Message::AddUrl)
                    } else {
                        self.error_modal = Some("Invalid url".to_string());
                        Task::none()
                    }
                } else {
                    self.error_modal = Some("Clipboard is empty".to_string());
                    Task::none()
                }
            }
            Message::AddUrl(index) => {
                // get input from index
                if let Some(input) = self.url_input.get_mut(index) {
                    // check if url is valid
                    if !self.url_regex.is_match(&input.value) {
                        input.status = UrlStatus::Invalid;
                        Task::none()
                    } else {
                        match input.status {
                            UrlStatus::Loading | UrlStatus::Loaded => Task::none(), // dont do anything if url is already loading or loaded
                            _ => {
                                input.status = UrlStatus::Loading; // set status to loading

                                Task::perform(
                                    get_files(self.mega.clone(), input.value.clone(), index),
                                    Message::GotFiles,
                                )
                            }
                        }
                    }
                } else {
                    self.error_modal = Some("An error occurred".to_string());
                    Task::none()
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

                Task::batch(commands)
            }
            Message::GotFiles(result) => {
                match result {
                    // files were loaded successfully
                    Ok((files, index)) => {
                        if let Some(input) = self.url_input.get_mut(index) {
                            input.status = UrlStatus::Loaded;

                            // Filter out duplicate files based on handles
                            for file in files {
                                // Collect all handles from this file and its children
                                let handles: Vec<String> =
                                    file.iter().map(|f| f.node.handle.clone()).collect();

                                // Check if any handle already exists
                                let has_duplicate = handles
                                    .iter()
                                    .any(|handle| self.file_handles.contains(handle));

                                // Only add if no duplicates found
                                if !has_duplicate {
                                    // Add all handles to the tracking set
                                    for handle in &handles {
                                        self.file_handles.insert(handle.clone());
                                    }
                                    // Add the file to the files list
                                    self.files.push(file);
                                }
                            }
                        } else {
                            self.error_modal = Some("An error occurred".to_string());
                        }
                    }
                    // an error occurred while loading the files
                    Err(index) => {
                        if let Some(input) = self.url_input.get_mut(index) {
                            input.status = UrlStatus::Invalid;
                        } else {
                            self.error_modal = Some("An error occurred".to_string());
                        }
                    }
                }

                Task::none()
            }
            Message::AddFiles => {
                // Collect handles from active downloads to prevent duplicates
                let active_handles: HashSet<String> =
                    self.active_downloads.keys().cloned().collect();

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

                // add downloads to queue
                for download in downloads {
                    self.download_sender.send(download).unwrap();
                }

                if self.worker.is_none() {
                    self.worker = Some(self.start_workers(self.config.max_workers));
                }

                self.route = Route::Home; // navigate to home
                Task::perform(async {}, |_| Message::ClearFiles) // clear files
            }
            Message::RunnerReady(sender) => {
                self.runner_sender = Some(sender);
                Task::none()
            }
            Message::Runner(message) => {
                match message {
                    RunnerMessage::Active(download) => {
                        // add download to active downloads
                        self.active_downloads
                            .insert(download.node.handle.clone(), download);
                    }
                    RunnerMessage::Inactive(id) => {
                        // add downloaded bytes to bandwidth counter
                        if let Some(download) = self.active_downloads.get(&id) {
                            self.bandwidth_counter += download.downloaded.load(Relaxed);
                        }

                        self.active_downloads.remove(&id); // remove download from active downloads

                        // if there are no active downloads, stop the runner
                        if self.active_downloads.is_empty() && self.download_receiver.is_empty() {
                            self.stop_workers();
                        }
                    }
                    RunnerMessage::Error(error) => {
                        self.errors.push(error);
                    }
                    RunnerMessage::Finished => (),
                }

                Task::none()
            }
            Message::Navigate(route) => {
                match route {
                    Route::Home | Route::Import | Route::Settings => self.route = route,
                    // only navigate to ChooseFiles if files are loaded
                    Route::ChooseFiles => {
                        if self.files.is_empty() {
                            self.error_modal = Some("No files imported".to_string())
                        } else {
                            self.route = route
                        }
                    }
                }

                Task::none()
            }
            Message::ToggleFile(item) => {
                // insert an entry for the file in the filter
                self.file_filter.insert(item.1.node.handle.clone(), item.0);

                // all children of the file should have the same entry in the filter
                item.1.iter().for_each(|file| {
                    self.file_filter.insert(file.node.handle.clone(), item.0);
                });

                Task::none()
            }
            Message::UrlInput((index, value)) => {
                if let Some(input) = self.url_input.get_mut(index) {
                    input.value = value; // update input value
                } else {
                    // if the input doesn't exist, create it
                    self.url_input.update(
                        index,
                        UrlInput {
                            value,
                            status: UrlStatus::None,
                        },
                    );
                }

                Task::none()
            }
            Message::ToggleExpanded(hash) => {
                if let Some(expanded) = self.expanded_files.get_mut(&hash) {
                    // toggle expanded state if it already exists
                    *expanded = !*expanded;
                } else {
                    // insert expanded state if it doesn't exist
                    self.expanded_files.insert(hash, true);
                }

                Task::none()
            }
            Message::AddInput => {
                self.url_input.insert(UrlInput {
                    value: String::new(),
                    status: UrlStatus::None,
                });

                Task::none()
            }
            Message::RemoveInput(index) => {
                self.url_input.remove(index);
                Task::none()
            }
            Message::CloseModal => {
                self.error_modal = None;
                Task::none()
            }
            Message::CancelDownloads => {
                // stop the workers
                self.stop_workers();
                // clear the queue
                while let Ok(Some(download)) = self.download_receiver.try_recv() {
                    download.cancel();
                }
                // cancel all active downloads
                for (_, download) in self.active_downloads.drain() {
                    download.cancel();
                }
                Task::none()
            }
            Message::CancelDownload(id) => {
                if let Some(download) = self.active_downloads.get(&id) {
                    download.cancel();
                }
                Task::none()
            }
            Message::PauseDownloads => {
                self.all_paused = true; // set all paused flag for UI purposes
                // pause each active download
                for (_, download) in self.active_downloads.iter() {
                    download.pause();
                }
                Task::none()
            }
            Message::PauseDownload(id) => {
                if let Some(download) = self.active_downloads.get(&id) {
                    download.pause();
                }
                Task::none()
            }
            Message::ResumeDownloads => {
                self.all_paused = false;
                for (_, download) in self.active_downloads.iter() {
                    download.resume();
                }
                Task::none()
            }
            Message::ResumeDownload(id) => {
                self.all_paused = false; // all downloads can't be paused if we're resuming one
                if let Some(download) = self.active_downloads.get(&id) {
                    download.resume();
                }
                Task::none()
            }
            Message::RebuildMega => {
                // if the worker is active, do not rebuild
                if self.worker.is_some() {
                    self.error_modal = Some(
                        "Cannot apply these configuration changes while downloads are active"
                            .to_string(),
                    );
                    return Task::none();
                }

                // build a new mega client
                match mega_builder(&self.config) {
                    Ok(mega) => {
                        self.mega = mega; // set the new mega client
                        self.rebuild_available = false; // rebuild is no longer available
                        Task::perform(async {}, |_| Message::SaveConfig) // save the config
                    }
                    Err(error) => {
                        self.error_modal = Some(format!("Failed to build mega client: {}", error));
                        Task::none()
                    }
                }
            }
            Message::SettingsSlider((index, value)) => {
                // update the config
                match index {
                    0 => {
                        if let Some(value) = value.to_usize() {
                            self.config.max_workers = value;
                        }
                    }
                    1 => {
                        if let Some(value) = value.to_usize() {
                            self.config.concurrency_budget = value;
                        }
                    }
                    2 => {
                        if let Some(value) = value.to_u64() {
                            self.config.timeout = Duration::from_millis(value);
                        }
                    }
                    3 => {
                        if let Some(value) = value.to_u32() {
                            self.config.max_retries = value;
                        }
                    }
                    4 => {
                        if let Some(value) = value.to_u64() {
                            self.config.min_retry_delay = Duration::from_millis(value);
                        }
                    }
                    5 => {
                        if let Some(value) = value.to_u64() {
                            self.config.max_retry_delay = Duration::from_millis(value);
                        }
                    }
                    _ => unreachable!(),
                }

                self.rebuild_available = true; // there are changes that can be applied now
                Task::none()
            }
            Message::SaveConfig => {
                for proxy in &self.config.proxies {
                    if !self.proxy_regex.is_match(proxy) {
                        self.error_modal = Some(format!("Invalid proxy url: {}", proxy));
                        return Task::none();
                    }
                }

                // save the config
                if let Err(error) = self.config.save() {
                    self.error_modal = Some(format!("Failed to save configuration: {}", error));
                }

                Task::none()
            }
            Message::ResetConfig => {
                self.config = Config::default();
                self.rebuild_available = true;
                Task::none()
            }
            Message::ThemeChanged(theme) => {
                self.config.set_theme(theme);
                Task::none()
            }
            Message::ProxyModeChanged(proxy_mode) => {
                if proxy_mode == ProxyMode::Single {
                    // if we're switching to single proxy mode, truncate the proxy list to 1
                    self.config.proxies.truncate(1);
                }
                self.config.proxy_mode = proxy_mode; // update the config
                self.rebuild_available = true; // there are changes that can be applied now
                Task::none()
            }
            Message::ProxyUrlChanged(value) => {
                if let Some(proxy_url) = self.config.proxies.get_mut(0) {
                    // update the proxy url
                    *proxy_url = value;
                } else {
                    // if there is no proxy url, add value to the proxy list
                    self.config.proxies.push(value);
                }
                self.rebuild_available = true; // there are changes that can be applied now
                Task::none()
            }
            Message::AddProxies => {
                if let Ok(Some(file_path)) = FileDialog::new()
                    .add_filter("Text File", &["txt"])
                    .show_open_single_file()
                {
                    match std::fs::File::open(file_path) {
                        Ok(mut file) => {
                            let mut contents = String::new();
                            file.read_to_string(&mut contents).unwrap();

                            for proxy in contents.lines() {
                                if self.proxy_regex.is_match(proxy) {
                                    self.config.proxies.push(proxy.to_string());
                                    self.rebuild_available = true;
                                }
                            }
                        }
                        Err(error) => {
                            self.error_modal = Some(format!("Failed to open file: {}", error));
                        }
                    };
                }

                Task::none()
            }
            Message::RemoveProxy(index) => {
                self.config.proxies.remove(index); // remove the proxy
                self.rebuild_available = true; // there are changes that can be applied now
                Task::none()
            }
            Message::ClearFiles => {
                self.files.clear(); // clear files
                self.file_filter.clear(); // clear file filter
                self.file_handles.clear(); // clear file handles tracking

                // clear loaded URL inputs
                self.url_input
                    .data
                    .retain(|_, input| input.status != UrlStatus::Loaded);

                // navigate to import if still on choose files
                if self.route == Route::ChooseFiles {
                    self.route = Route::Import;
                }

                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        // build content
        let content = match self.route {
            Route::Home => {
                let mut download_list = Column::new();

                for (index, (id, download)) in self.active_downloads.iter().enumerate() {
                    let mut progress = download.progress();

                    if progress < 0.1 && progress > 0_f32 {
                        progress = 0.1;
                    }

                    let pause_button = if download.is_paused() {
                        icon_button(PLAY_ICON, Message::ResumeDownload(id.clone()))
                    } else {
                        icon_button(PAUSE_ICON, Message::PauseDownload(id.clone()))
                    };

                    download_list = download_list.push(
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
                                        .style(progress_bar::danger)
                                        .length(Length::Fixed(80_f32))
                                        .girth(Length::Fixed(15_f32)),
                                )
                                .push(space::horizontal().width(Length::Fixed(10_f32)))
                                .push(
                                    text(
                                        format!("{} MB/s", pad_f32(download.speed()))
                                            .replace('0', "O"),
                                    )
                                    .width(Length::Shrink)
                                    .height(Length::Fill)
                                    .align_y(Vertical::Center)
                                    .font(MONOSPACE)
                                    .size(16),
                                )
                                .push(space::horizontal().width(Length::Fixed(5_f32)))
                                .push(icon_button(X_ICON, Message::CancelDownload(id.clone())))
                                .push(pause_button)
                                .push(space::horizontal().width(Length::Fixed(7_f32))),
                        )
                        .style(move |theme| styles::container::Download { index }.style(theme)),
                    );
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

                let mut download_group = Column::new().push(
                    scrollable(download_list)
                        // .(Properties::default().width(5).scroller_width(5).margin(0))
                        .height(Length::Fill),
                );

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
                                    .style(button::danger)
                            } else {
                                button(" Pause All ")
                                    .on_press(Message::PauseDownloads)
                                    .style(button::danger)
                            })
                            .push(
                                button(" Cancel All ")
                                    .on_press(Message::CancelDownloads)
                                    .style(button::warning),
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
            }
            Route::Import => container(
                Column::new()
                    .spacing(5)
                    .push(scrollable(self.url_inputs()).height(Length::Fill))
                    .push(
                        Row::new()
                            .spacing(10)
                            .push(
                                button(" Add from clipboard ")
                                    .style(button::primary)
                                    .on_press(Message::AddUrlClipboard),
                            )
                            .push(
                                button(" + ")
                                    .style(button::primary)
                                    .on_press(Message::AddInput),
                            )
                            .push(
                                button(" Load all ")
                                    .style(button::primary)
                                    .on_press(Message::AddAllUrls),
                            ),
                    ),
            ),
            Route::ChooseFiles => {
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
                                        .style(button::primary)
                                        .on_press(Message::AddFiles),
                                )
                                .push(
                                    button(" Cancel ")
                                        .style(button::danger)
                                        .on_press(Message::ClearFiles),
                                )
                                .push(
                                    container(
                                        text(format!(" {:.2} GB ", size_gb).replace('0', "O"))
                                            .font(MONOSPACE)
                                            .align_y(Vertical::Center)
                                            .align_x(Horizontal::Center)
                                            .width(Length::Fill)
                                            .height(Length::Fill),
                                    )
                                    .style(|theme: &Theme| {
                                        let palette = theme.extended_palette();
                                        container::Style {
                                            background: Some(
                                                palette.background.strong.color.into(),
                                            ),
                                            border: Border::default().rounded(4.0),
                                            ..Default::default()
                                        }
                                    })
                                    .height(Length::Fill),
                                ),
                        ),
                )
            }
            Route::Settings => {
                let mut apply_button = button(" Apply ").style(button::primary);

                if self.rebuild_available {
                    apply_button = apply_button.on_press(Message::RebuildMega);
                }

                container(
                    Column::new()
                        .width(Length::Fixed(350_f32))
                        .push(self.settings_slider(
                            0,
                            self.config.max_workers,
                            1_f64..=10_f64,
                            "Max Workers:",
                        ))
                        .push(self.settings_slider(
                            1,
                            self.config.concurrency_budget,
                            1_f64..=100_f64,
                            "Concurrency Budget:",
                        ))
                        .push(self.settings_slider(
                            2,
                            self.config.timeout.as_millis() as usize,
                            100_f64..=60000_f64,
                            "Timeout:",
                        ))
                        .push(self.settings_slider(
                            3,
                            self.config.max_retries as usize,
                            1_f64..=10_f64,
                            "Max retries:",
                        ))
                        .push(self.settings_slider(
                            4,
                            self.config.min_retry_delay.as_millis() as usize,
                            100_f64..=self.config.max_retry_delay.as_millis() as f64,
                            "Min Retry delay:",
                        ))
                        .push(self.settings_slider(
                            5,
                            self.config.max_retry_delay.as_millis() as usize,
                            self.config.min_retry_delay.as_millis() as f64..=60000_f64,
                            "Max Retry delay:",
                        ))
                        .push(space::vertical().height(Length::Fixed(10_f32)))
                        .push(
                            Row::new()
                                .height(Length::Fixed(30_f32))
                                .push(space::horizontal().width(Length::Fixed(8_f32)))
                                .push(text("Theme").align_y(Vertical::Center).height(Length::Fill))
                                .push(space::horizontal())
                                .push(
                                    pick_list(
                                        Theme::ALL,
                                        Some(self.config.get_theme()),
                                        Message::ThemeChanged,
                                    )
                                    .width(Length::Fixed(170_f32)),
                                ),
                        )
                        .push(space::vertical().height(Length::Fixed(10_f32)))
                        .push(self.settings_picklist(
                            "Proxy Mode",
                            &ProxyMode::ALL[..],
                            Some(self.config.proxy_mode),
                            Message::ProxyModeChanged,
                        ))
                        .push(space::vertical().height(Length::Fixed(10_f32)))
                        .push(self.proxy_selector())
                        .push(space::vertical().height(Length::Fill))
                        .push(
                            Row::new()
                                .push(space::horizontal().width(Length::Fixed(8_f32)))
                                .push(
                                    button(" Save ")
                                        .style(button::primary)
                                        .on_press(Message::SaveConfig),
                                )
                                .push(space::horizontal().width(Length::Fixed(10_f32)))
                                .push(apply_button)
                                .push(space::horizontal().width(Length::Fixed(10_f32)))
                                .push(
                                    button(" Reset ")
                                        .style(button::warning)
                                        .on_press(Message::ResetConfig),
                                ),
                        ),
                )
            }
        };

        // nav + content = body
        let nav_theme = self.config.get_theme();
        let body = container(
            Row::new()
                .push(
                    container(
                        Column::new()
                            .padding(4)
                            .spacing(4)
                            .push(self.nav_button(&nav_theme, "Home", Route::Home, false))
                            .push(self.nav_button(&nav_theme, "Import", Route::Import, false))
                            .push(self.nav_button(
                                &nav_theme,
                                "Choose files",
                                Route::ChooseFiles,
                                self.files.is_empty(),
                            ))
                            .push(space::vertical().height(Length::Fill))
                            .push(self.nav_button(&nav_theme, "Settings", Route::Settings, false)),
                    )
                    .width(Length::Fixed(170_f32))
                    .height(Length::Fill)
                    .style(|theme: &Theme| {
                        let palette = theme.extended_palette();
                        container::Style {
                            background: Some(palette.background.strong.color.into()),
                            ..Default::default()
                        }
                    }),
                )
                .push(content.padding(10).width(Length::Fill)),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        if let Some(error_message) = &self.error_modal {
            let theme = self.config.get_theme();
            let error_color = theme.extended_palette().danger.strong.color;
            stack![
                body,
                opaque(
                    mouse_area(
                        center(opaque(
                            container(
                                Column::new()
                                    .spacing(5)
                                    .push(
                                        text(error_message)
                                            .color(error_color)
                                            .align_y(Vertical::Center)
                                            .align_x(Horizontal::Center),
                                    )
                                    .push(space::horizontal().width(Length::Fixed(100_f32)))
                                    .push(
                                        Row::new()
                                            .spacing(5)
                                            .push(space::horizontal().width(Length::FillPortion(3)))
                                            .push(
                                                button(" Ok ")
                                                    .style(button::primary)
                                                    .on_press(Message::CloseModal),
                                            ),
                                    ),
                            )
                            .width(Length::Fixed(150_f32))
                            .padding(10)
                            .style(container::rounded_box)
                        ))
                        .style(|_theme| container::Style {
                            background: Some(
                                Color {
                                    a: 0.5,
                                    ..Color::BLACK
                                }
                                .into(),
                            ),
                            ..container::Style::default()
                        })
                    )
                    .on_press(Message::CloseModal)
                )
            ]
            .into()
        } else {
            body.into()
        }
    }

    // determines the correct Theme based on configuration & system settings
    fn theme(&self) -> Option<Theme> {
        // Return None for system theme (Iced 0.14 handles this automatically)
        // For explicit theme selection, return Some(theme)
        Some(self.config.get_theme())
    }

    fn subscription(&self) -> Subscription<Message> {
        // reads runner messages from channel and sends them to the UI
        let runner_subscription = Subscription::run(runner_worker);

        // forces the UI to refresh every second
        // this is needed because changes to the active downloads don't trigger a refresh
        let refresh = every(Duration::from_secs(1)).map(|_| Message::Refresh);

        // run all subscriptions in parallel
        Subscription::batch(vec![runner_subscription, refresh])
    }

    fn recursive_files<'a>(&self, file: &'a MegaFile) -> Element<'a, Message> {
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
                        .style(checkbox::danger),
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
                            .width(Length::Fixed(16_f32)),
                        )
                        .style(button::background)
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
                            .style(checkbox::danger),
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

    fn nav_button<'a>(
        &self,
        theme: &Theme,
        label: &'a str,
        route: Route,
        disabled: bool,
    ) -> Element<'a, Message> {
        let palette = theme.extended_palette();
        let style = if disabled {
            SvgIcon::new(palette.secondary.weak.color.into())
        } else {
            SvgIcon::new(palette.primary.strong.color.into())
        };

        let mut row = Row::new()
            .align_y(Alignment::Center)
            .height(Length::Fixed(40_f32));

        if self.route == route {
            let style = style.clone();
            row = row
                .push(
                    svg(svg::Handle::from_memory(SELECTED_ICON))
                        .style(move |theme, status| style.style(theme, status))
                        .width(Length::Fixed(4_f32))
                        .height(Length::Fixed(25_f32)),
                )
                .push(space::horizontal().width(Length::Fixed(8_f32)))
        } else {
            row = row.push(space::horizontal().width(Length::Fixed(12_f32)))
        }

        let handle = match route {
            Route::Home => svg::Handle::from_memory(HOME_ICON),
            Route::Import => svg::Handle::from_memory(IMPORT_ICON),
            Route::ChooseFiles => svg::Handle::from_memory(CHOOSE_ICON),
            Route::Settings => svg::Handle::from_memory(SETTINGS_ICON),
        };

        row = row
            .push(
                container(
                    svg(handle)
                        .width(Length::Fixed(28_f32))
                        .height(Length::Fixed(28_f32))
                        .style(move |theme, status| style.style(theme, status)),
                )
                .padding(4)
                .style({
                    let is_active = self.route == route;
                    move |theme| styles::container::Icon::new(is_active).style(theme)
                }),
            )
            .push(space::horizontal().width(Length::Fixed(12_f32)));

        let nav_style = styles::button::Nav {
            active: self.route == route,
        };
        let mut button = button(row.push(text(label)))
            .style(move |theme, status| nav_style.style(theme, status))
            .width(Length::Fill)
            .padding(0);

        if !disabled {
            button = button.on_press(Message::Navigate(route));
        }

        button.into()
    }

    fn error_log(&self) -> Element<'_, Message> {
        let theme = self.config.get_theme();
        let error_color = theme.extended_palette().danger.strong.color;
        let mut column = Column::new().spacing(2).width(Length::Fill);

        for error in &self.errors {
            column = column.push(text(error).color(error_color));
        }

        column.into()
    }

    fn url_inputs(&self) -> Element<'_, Message> {
        let mut inputs = Column::new().spacing(5);

        for (index, input) in self.url_input.data.iter() {
            let url_input_style = styles::text_input::UrlInput { mode: input.status };
            let mut text_input = text_input("Url", &input.value)
                .style(move |theme, status| url_input_style.style(theme, status))
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
                                .height(Length::Fixed(22_f32)),
                        )
                        .style(button::background)
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
                                .height(Length::Fixed(26_f32)),
                        )
                        .padding(2),
                    );
                }
            }

            inputs = inputs.push(row);
        }

        inputs.into()
    }

    fn settings_slider<'a>(
        &self,
        index: usize,
        value: usize,
        range: RangeInclusive<f64>,
        label: &'a str,
    ) -> Element<'a, Message> {
        Row::new()
            .height(Length::Fixed(30_f32))
            .push(space::horizontal().width(Length::Fixed(8_f32)))
            .push(text(label).align_y(Vertical::Center).height(Length::Fill))
            .push(space::horizontal())
            .push(
                text(pad_usize(value).replace('0', "O"))
                    .font(MONOSPACE)
                    .align_y(Vertical::Center)
                    .height(Length::Fill)
                    .size(20),
            )
            .push(space::horizontal().width(Length::Fixed(10_f32)))
            .push(
                slider(range, value as f64, move |value| {
                    Message::SettingsSlider((index, value))
                })
                .width(Length::Fixed(130_f32))
                .height(30)
                .style(styles::slider::slider_style),
            )
            .into()
    }

    fn settings_picklist<'a, T>(
        &self,
        label: &'a str,
        options: impl Into<Cow<'a, [T]>> + std::borrow::Borrow<[T]> + 'a,
        selected: Option<T>,
        message: fn(T) -> Message,
    ) -> Element<'a, Message>
    where
        T: ToString + Eq + 'static + Clone,
        [T]: ToOwned<Owned = Vec<T>>,
    {
        Row::new()
            .height(Length::Fixed(30_f32))
            .push(space::horizontal().width(Length::Fixed(8_f32)))
            .push(text(label).align_y(Vertical::Center).height(Length::Fill))
            .push(space::horizontal())
            .push(pick_list(options, selected, message).width(Length::Fixed(170_f32)))
            .into()
    }

    fn proxy_selector(&self) -> Element<'_, Message> {
        let mut column = Column::new();

        if self.config.proxy_mode == ProxyMode::Random {
            let mut proxy_display = Column::new().width(Length::Fill);

            for (index, proxy) in self.config.proxies.iter().enumerate() {
                proxy_display = proxy_display.push(
                    container(
                        Row::new()
                            .padding(4)
                            .push(text(proxy))
                            .push(space::horizontal())
                            .push(
                                button(
                                    svg(svg::Handle::from_memory(X_ICON))
                                        .width(Length::Fixed(15_f32))
                                        .height(Length::Fixed(15_f32)),
                                )
                                .style(button::background)
                                .on_press(Message::RemoveProxy(index))
                                .padding(4),
                            )
                            .push(space::horizontal().width(Length::Fixed(8_f32))),
                    )
                    .style(move |theme: &Theme| styles::container::Download { index }.style(theme))
                    .width(Length::Fill),
                )
            }

            if self.config.proxies.is_empty() {
                proxy_display = proxy_display.push(
                    text("No proxies")
                        .width(Length::Fixed(100_f32))
                        .height(Length::Fixed(35_f32))
                        .align_y(Vertical::Center)
                        .align_x(Horizontal::Center),
                );
            }

            column = column.push(
                container(
                    Column::new()
                        .push(scrollable(proxy_display).height(Length::Fixed(125_f32)))
                        .push(space::vertical())
                        .push(
                            container(
                                button(" Add proxies ")
                                    .on_press(Message::AddProxies)
                                    .style(button::danger)
                                    .padding(4),
                            )
                            .padding(5),
                        ),
                )
                .style(container::bordered_box)
                .height(Length::Fixed(170_f32))
                .padding(2),
            );
        } else if self.config.proxy_mode == ProxyMode::Single {
            column = column.push(
                text_input(
                    "Proxy url",
                    self.config.proxies.first().unwrap_or(&String::new()),
                )
                .on_input(Message::ProxyUrlChanged)
                .style(|theme, status| {
                    styles::text_input::UrlInput {
                        mode: UrlStatus::None,
                    }
                    .style(theme, status)
                })
                .padding(6),
            );
        }

        Row::new()
            .push(space::horizontal().width(Length::Fixed(8_f32)))
            .push(column)
            .into()
    }

    fn start_workers(&self, workers: usize) -> WorkerState {
        let cancel = CancellationToken::new();
        let runner_sender = self
            .runner_sender
            .clone()
            .expect("Runner sender not available - subscription may not be ready");
        WorkerState {
            handles: spawn_workers(
                self.mega.clone(),
                Arc::new(self.config.clone()),
                self.download_receiver.clone(),
                self.download_sender.clone_async(),
                runner_sender,
                cancel.clone(),
                workers,
            ),
            cancel,
        }
    }

    fn stop_workers(&mut self) {
        if let Some(state) = self.worker.take() {
            state.cancel.cancel();

            // join workers in the background to log errors
            tokio::spawn(async move {
                for result in join_all(state.handles).await {
                    match result {
                        Err(error) => error!("worker panicked: {error:?}"),
                        Ok(Err(error)) => error!("worker failed: {error:?}"),
                        Ok(Ok(())) => (),
                    }
                }
            });
        }
    }
}

impl From<Config> for App {
    /// initializes the app from the config
    fn from(config: Config) -> Self {
        // build the mega client
        let mega = mega_builder(&config).unwrap();
        let (download_sender, download_receiver) = kanal::unbounded();

        Self {
            config,
            mega,
            worker: None,
            active_downloads: HashMap::new(),
            runner_sender: None,
            download_sender,
            download_receiver: download_receiver.to_async(),
            files: Vec::new(),
            file_filter: HashMap::new(),
            file_handles: HashSet::new(),
            url_input: IndexMap::default(),
            expanded_files: HashMap::new(),
            route: Route::Home,
            url_regex: Regex::new("https?://mega\\.nz/(folder|file)/([\\dA-Za-z]+)#([\\dA-Za-z-_]+)").unwrap(),
            proxy_regex: Regex::new("(?:(?:https?|socks5h?)://)(?:(?:[a-zA-Z\\d]+(?::[a-zA-Z\\d]+)?@)?)(?:(?:[a-z\\d](?:[a-z\\d\\-]{0,61}[a-z\\d])?\\.)+[a-z\\d][a-z\\d\\-]{0,61}[a-z\\d]|(?:\\d{1,3}\\.){3}\\d{1,3})(:\\d{1,5})").unwrap(),
            errors: Vec::new(),
            error_modal: None,
            all_paused: false,
            bandwidth_counter: 0,
            rebuild_available: false,
        }
    }
}

/// builds the iced app
pub(crate) fn build_app() -> iced::Application<impl iced::Program<Message = Message, Theme = Theme>>
{
    iced::application(App::new, App::update, App::view)
        .title(App::title)
        .subscription(App::subscription)
        .theme(App::theme)
        .window_size((700.0, 550.0))
        .font(CABIN_REGULAR)
        .font(INCONSOLATA_MEDIUM)
        .default_font(Font::with_name("Cabin"))
}
