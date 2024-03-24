use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::io::Read;
use std::ops::RangeInclusive;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Duration;

use iced::alignment::{Horizontal, Vertical};
use iced::time::every;
use iced::widget::{canvas, svg, Column, Row, Space, Text};
use iced::window::{PlatformSpecific, Settings as Window};
use iced::{
    clipboard, executor, theme, Alignment, Application, Color, Command, Element, Font, Length,
    Renderer, Settings, Subscription,
};
use iced_native::widget::scrollable::Properties;
use iced_native::widget::{
    button, checkbox, container, pick_list, progress_bar, scrollable, text, text_input,
};
use mega::Client;
use native_dialog::FileDialog;
use num_traits::cast::ToPrimitive;
use regex::Regex;
use reqwest::{Client as HttpClient, Proxy, Url};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::config::Config;
use crate::loading_wheel::LoadingWheel;
use crate::modal::Modal;
use crate::slider::Slider;
use crate::{
    get_files, runner, styles, Download, DownloadQueue, MegaFile, ProxyMode, RunnerMessage,
};

const CHECK_ICON: &[u8] = include_bytes!("../assets/check.svg");
const COLLAPSE_ICON: &[u8] = include_bytes!("../assets/collapse.svg");
const EXPAND_ICON: &[u8] = include_bytes!("../assets/expand.svg");
const SELECTED_ICON: &[u8] = include_bytes!("../assets/selector.svg");
const IMPORT_ICON: &[u8] = include_bytes!("../assets/import.svg");
const CHOOSE_ICON: &[u8] = include_bytes!("../assets/choose.svg");
const SETTINGS_ICON: &[u8] = include_bytes!("../assets/settings.svg");
const HOME_ICON: &[u8] = include_bytes!("../assets/home.svg");
const TRASH_ICON: &[u8] = include_bytes!("../assets/trash.svg");
const X_ICON: &[u8] = include_bytes!("../assets/x.svg");
const PAUSE_ICON: &[u8] = include_bytes!("../assets/pause.svg");
const PLAY_ICON: &[u8] = include_bytes!("../assets/play.svg");

const INCONSOLATA_MEDIUM: &[u8] =
    include_bytes!("../assets/Inconsolata/static/Inconsolata-Medium.ttf");
const CABIN_REGULAR: &[u8] = include_bytes!("../assets/Cabin/static/Cabin-Regular.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) enum Theme {
    // use system theme
    System,
    // force dark theme
    Dark,
    // fore light theme
    Light,
}

impl Theme {
    pub const ALL: [Self; 3] = [Self::System, Self::Dark, Self::Light];
}

// implement display for theme dropdown
impl Display for Theme {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::System => "System Default",
                Self::Dark => "Dark",
                Self::Light => "Light",
            }
        )
    }
}

#[derive(Debug, Clone)]
pub(crate) enum MegaError {
    OutOfBandwidth,
    // arc is needed to clone mega::Error
    Other(Arc<mega::Error>),
}

impl From<mega::Error> for MegaError {
    fn from(e: mega::Error) -> Self {
        match e {
            mega::Error::OutOfBandwidth => Self::OutOfBandwidth,
            _ => Self::Other(Arc::new(e)),
        }
    }
}

impl Display for MegaError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBandwidth => write!(f, "Out of bandwidth"),
            Self::Other(e) => write!(f, "{}", e),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    // force the GUI to update
    Refresh,
    // add url from clipboard
    AddUrlClipboard,
    // got clipboard contents
    GotClipboard(Option<String>),
    // url added by user
    AddUrl(usize),
    // add all the urls
    AddAllUrls,
    // backend got files for url
    GotFiles(Result<(Vec<MegaFile>, usize), usize>),
    // user added files to download queue
    AddFiles,
    // received message from runner
    Runner(RunnerMessage),
    // received message from mega client
    Mega(MegaError),
    // navigate to a different route
    Navigate(Route),
    // toggle file & children for download
    ToggleFile((bool, MegaFile)),
    // when a character is changed in the url input
    UrlInput((usize, String)),
    // toggle expanded state of file tree
    ToggleExpanded(String),
    // create a new url input
    AddInput,
    // remove a url input
    RemoveInput(usize),
    // tick for loading spinner
    Tick(usize),
    // close the error modal
    CloseModal,
    // cancel all downloads
    CancelDownloads,
    // cancel download by id
    CancelDownload(String),
    // pause all downloads
    PauseDownloads,
    // pause download by id
    PauseDownload(String),
    // resume all downloads
    ResumeDownloads,
    // resume download by id
    ResumeDownload(String),
    // rebuild mega client with new config
    RebuildMega,
    // when a settings slider is changed, usize is index
    SettingsSlider((usize, f64)),
    // save current config to disk
    SaveConfig,
    // reset config to default
    ResetConfig,
    // theme changed
    ThemeChanged(Theme),
    // proxy mode changed
    ProxyModeChanged(ProxyMode),
    // proxy url changed, single proxy mode
    ProxyUrlChanged(String),
    // add proxies from file
    AddProxies,
    // remove proxy
    RemoveProxy(usize),
    // remove any loaded files
    ClearFiles,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Route {
    Home,
    Import,
    ChooseFiles,
    Settings,
}

struct UrlInput {
    value: String,
    status: UrlStatus,
}

impl Default for UrlInput {
    fn default() -> Self {
        Self {
            value: String::new(),
            status: UrlStatus::None,
        }
    }
}

#[derive(PartialEq, Clone, Copy)]
pub(crate) enum UrlStatus {
    None,
    Invalid,
    // f32 is the angle of the loading spinner
    Loading(f32),
    Loaded,
}

pub(crate) struct App {
    config: Config,
    mega: Client,
    runner: Option<Vec<JoinHandle<()>>>,
    download_queue: DownloadQueue,
    queued: Arc<AtomicUsize>,
    active_downloads: HashMap<String, Download>,
    runner_sender: Arc<UnboundedSender<RunnerMessage>>,
    runner_receiver: RefCell<Option<UnboundedReceiver<RunnerMessage>>>,
    mega_sender: Arc<UnboundedSender<mega::Error>>,
    mega_receiver: RefCell<Option<UnboundedReceiver<mega::Error>>>,
    files: Vec<MegaFile>,
    file_filter: HashMap<String, bool>,
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
    active_threads: Arc<AtomicUsize>,
}

impl Application for App {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = theme::Theme;
    type Flags = Config;

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        (flags.into(), Command::none())
    }

    fn title(&self) -> String {
        let mut title = String::from("Giga Grabber");

        // runner is None when not in use
        if self.runner.is_some() {
            title.push_str(&format!(
                " - downloads active - {} threads active",
                self.active_threads.load(Relaxed)
            ));
        }

        if !self.active_downloads.is_empty() {
            title.push_str(&format!(" - {} running", self.active_downloads.len()));
        }

        let queued = self.queued.load(Relaxed);
        if queued > 0 {
            title.push_str(&format!(" - {} queued", queued));
        }

        title
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            // force the GUI to update
            Message::Refresh => Command::none(),
            // add url from clipboard, emit GotClipboard w/ contents
            Message::AddUrlClipboard => clipboard::read(Message::GotClipboard),
            // got clipboard contents
            Message::GotClipboard(contents) => {
                if let Some(url) = contents {
                    if self.url_regex.is_match(&url) {
                        // create new url input with url as value
                        let index = self.url_input.insert(UrlInput {
                            value: url.clone(),
                            status: UrlStatus::None,
                        });

                        // load the url
                        Command::perform(async move { index }, Message::AddUrl)
                    } else {
                        self.error_modal = Some("Invalid url".to_string());
                        Command::none()
                    }
                } else {
                    self.error_modal = Some("Clipboard is empty".to_string());
                    Command::none()
                }
            }
            // load files from url
            Message::AddUrl(index) => {
                // get input from index
                if let Some(input) = self.url_input.get_mut(index) {
                    // check if url is valid
                    if !self.url_regex.is_match(&input.value) {
                        input.status = UrlStatus::Invalid;
                        Command::none()
                    } else {
                        match input.status {
                            UrlStatus::Loading(_) | UrlStatus::Loaded => Command::none(), // dont do anything if url is already loading or loaded
                            _ => {
                                input.status = UrlStatus::Loading(0_f32); // set status to loading

                                Command::batch(vec![
                                    // get files from url asynchronously
                                    Command::perform(
                                        get_files(self.mega.clone(), input.value.clone(), index),
                                        Message::GotFiles,
                                    ),
                                    // begin the loading spinner animation
                                    Command::perform(
                                        async move {
                                            // wait 10ms before ticking
                                            sleep(Duration::from_millis(10)).await;
                                            index
                                        },
                                        Message::Tick,
                                    ),
                                ])
                            }
                        }
                    }
                } else {
                    self.error_modal = Some("An error occurred".to_string());
                    Command::none()
                }
            }
            // perform AddUrl for every url input
            Message::AddAllUrls => {
                let commands: Vec<_> = self
                    .url_input
                    .data
                    .keys()
                    .cloned()
                    .map(|index| Command::perform(async move { index }, Message::AddUrl))
                    .collect();

                Command::batch(commands)
            }
            // mega files were loaded from url
            Message::GotFiles(result) => {
                match result {
                    // files were loaded successfully
                    Ok((files, index)) => {
                        if let Some(input) = self.url_input.get_mut(index) {
                            input.status = UrlStatus::Loaded;
                            self.files.extend(files);
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

                Command::none()
            }
            // add loaded files to download queue
            Message::AddFiles => {
                // flatten file structure into a list of downloads
                let downloads = self
                    .files
                    .iter()
                    .flat_map(|file| file.iter())
                    .filter(|file| {
                        if let Some(download) = self.file_filter.get(file.node.hash()) {
                            *download && file.node.kind().is_file()
                        } else {
                            file.node.kind().is_file()
                        }
                    }) // only download files that are marked for download
                    .map(|file| file.clone().into()) // convert MegaFile into Download
                    .collect::<Vec<Download>>();

                // update queued count
                self.queued.fetch_add(downloads.len(), Relaxed);
                // add downloads to queue
                for download in downloads {
                    self.download_queue.push(download);
                }

                if let Some(runner_threads) = self.runner.as_ref() {
                    if runner_threads.iter().all(|thread| thread.is_finished()) {
                        // all threads are finished, create new runner
                        self.runner = Some(self.start_runner(self.config.max_concurrent_files));
                    } else {
                        // the number of finished threads
                        let finished = runner_threads
                            .iter()
                            .filter(|thread| thread.is_finished())
                            .count();

                        if finished > 0 {
                            // there are finished threads, create replacements
                            let new_threads = self.start_runner(finished);

                            if let Some(mut_threads) = self.runner.as_mut() {
                                mut_threads.extend(new_threads);
                            }
                        }
                    }
                } else {
                    // runner doesn't exist, create it
                    self.runner = Some(self.start_runner(self.config.max_concurrent_files));
                }

                self.route = Route::Home; // navigate to home
                Command::perform(async {}, |_| Message::ClearFiles) // clear files
            }
            // a download has either been started or stopped
            Message::Runner(message) => {
                match message {
                    RunnerMessage::Start(download) => {
                        // add download to active downloads
                        self.active_downloads
                            .insert(download.node.hash().to_string(), download);
                    }
                    RunnerMessage::Stop(id) => {
                        // add downloaded bytes to bandwidth counter
                        if let Some(download) = self.active_downloads.get(&id) {
                            self.bandwidth_counter += download.downloaded.load(Relaxed);
                        }

                        self.active_downloads.remove(&id); // remove download from active downloads

                        // if there are no active downloads, abort runner
                        if self.active_downloads.is_empty() {
                            self.stop_runner();
                        }
                    }
                }

                Command::none()
            }
            // a message (error) from the mega backend
            Message::Mega(error) => {
                match error {
                    MegaError::OutOfBandwidth => {
                        if !self.all_paused {
                            self.error_modal = Some("Out of bandwidth".to_string());
                            Command::perform(async {}, |_| Message::PauseDownloads)
                            // pause downloads
                        } else {
                            Command::none()
                        }
                    }
                    _ => {
                        self.errors.push(format!("{}", error)); // add error to error list
                        Command::none()
                    }
                }
            }
            // navigate to a route
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

                Command::none()
            }
            // toggle whether a file should be downloaded
            Message::ToggleFile((checked, file)) => {
                // insert an entry for the file in the filter
                self.file_filter
                    .insert(file.node.hash().to_string(), checked);

                // all children of the file should be have the same entry in the filter
                file.iter().for_each(|file| {
                    self.file_filter
                        .insert(file.node.hash().to_string(), checked);
                });

                Command::none()
            }
            // text changed in url input
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

                Command::none()
            }
            // expand a folder in the file tree
            Message::ToggleExpanded(hash) => {
                if let Some(expanded) = self.expanded_files.get_mut(&hash) {
                    // toggle expanded state if it already exists
                    *expanded = !*expanded;
                } else {
                    // insert expanded state if it doesn't exist
                    self.expanded_files.insert(hash, true);
                }

                Command::none()
            }
            // create a new url input
            Message::AddInput => {
                self.url_input.insert(UrlInput {
                    value: String::new(),
                    status: UrlStatus::None,
                });

                Command::none()
            }
            // remove a url input
            Message::RemoveInput(index) => {
                self.url_input.remove(index);
                Command::none()
            }
            // loop that makes spinner spin
            Message::Tick(index) => {
                match self.url_input.data.get_mut(&index).unwrap().status {
                    // if input is still loading, update angle
                    UrlStatus::Loading(ref mut angle) => {
                        *angle += 0.2;
                        // continue the loop
                        Command::perform(
                            async move {
                                sleep(Duration::from_millis(10)).await;
                                index
                            },
                            Message::Tick,
                        )
                    }
                    // otherwise break the loop
                    _ => Command::none(),
                }
            }
            // close error modal
            Message::CloseModal => {
                self.error_modal = None;
                Command::none()
            }
            // cancels all downloads
            Message::CancelDownloads => {
                // clear the queue
                while self.download_queue.try_pop().is_some() {}

                // cancel all active downloads
                for download in self.active_downloads.values() {
                    download.cancel();
                }

                self.stop_runner(); // stop the runner
                self.active_downloads.clear(); // clear active downloads
                self.queued.store(0, Relaxed); // reset queued counter

                Command::none()
            }
            // cancels a download
            Message::CancelDownload(id) => {
                if let Some(download) = self.active_downloads.get(&id) {
                    download.cancel();
                }

                Command::none()
            }
            // pauses all downloads
            Message::PauseDownloads => {
                self.all_paused = true; // set all paused flag for gui purposes

                // pause each active download
                for (_, download) in self.active_downloads.iter() {
                    download.pause();
                }

                Command::none()
            }
            // pauses a download
            Message::PauseDownload(id) => {
                if let Some(download) = self.active_downloads.get(&id) {
                    download.pause();
                }

                Command::none()
            }
            // resumes all downloads
            Message::ResumeDownloads => {
                self.all_paused = false; // unset all paused flag for gui purposes

                // resume each active (paused?) download
                for (_, download) in self.active_downloads.iter() {
                    download.resume();
                }

                Command::none()
            }
            // resumes a download
            Message::ResumeDownload(id) => {
                self.all_paused = false; // all downloads can't be paused if we're resuming one

                if let Some(download) = self.active_downloads.get(&id) {
                    download.resume();
                }

                Command::none()
            }
            // rebuilds the mega client
            Message::RebuildMega => {
                if let Some(runner_threads) = &self.runner {
                    // if any threads are active, we can't rebuild
                    if runner_threads.iter().any(|thread| !thread.is_finished()) {
                        self.error_modal = Some(
                            "Cannot apply these configuration changes while downloads are active"
                                .to_string(),
                        );
                        return Command::none();
                    } else {
                        // deletes the inactive runner
                        self.stop_runner();
                    }
                }

                // build a new mega client
                match mega_builder(&self.config, Arc::clone(&self.mega_sender)) {
                    Ok(mega) => {
                        self.mega = mega; // set the new mega client
                        self.rebuild_available = false; // rebuild is no longer available
                        Command::perform(async {}, |_| Message::SaveConfig) // save the config
                    }
                    Err(error) => {
                        // show error modal
                        self.error_modal = Some(format!("Failed to build mega client: {}", error));
                        Command::none()
                    }
                }
            }
            // a settings slider has been moved
            Message::SettingsSlider((index, value)) => {
                self.rebuild_available = true; // there are changes that can be applied now

                // update the config
                match index {
                    0 => {
                        if let Some(value) = value.to_usize() {
                            self.config.max_threads = value;

                            if self.config.max_threads_per_file > value {
                                self.config.max_threads_per_file = value;
                            }

                            if self.config.max_concurrent_files > value {
                                self.config.max_concurrent_files = value;
                            }
                        }
                    }
                    1 => {
                        if let Some(value) = value.to_usize() {
                            self.config.max_concurrent_files = value;
                        }
                    }
                    2 => {
                        if let Some(value) = value.to_usize() {
                            self.config.max_threads_per_file = value;
                        }
                    }
                    3 => {
                        if let Some(value) = value.to_u64() {
                            self.config.timeout = value;
                        }
                    }
                    4 => {
                        if let Some(value) = value.to_usize() {
                            self.config.max_retries = value;
                        }
                    }
                    5 => {
                        if let Some(value) = value.to_u64() {
                            self.config.min_retry_delay = value;
                        }
                    }
                    6 => {
                        if let Some(value) = value.to_u64() {
                            self.config.max_retry_delay = value;
                        }
                    }
                    _ => unreachable!(),
                }

                Command::none()
            }
            // save the config
            Message::SaveConfig => {
                // TODO should something else happen if a proxy is invalid?
                for proxy in &self.config.proxies {
                    if !self.proxy_regex.is_match(proxy) {
                        self.error_modal = Some(format!("Invalid proxy url: {}", proxy));
                    }
                }

                // save the config
                if let Err(error) = self.config.save() {
                    self.error_modal = Some(format!("Failed to save configuration: {}", error));
                }

                Command::none()
            }
            // reset the config to default values
            Message::ResetConfig => {
                self.config = Config::default();

                Command::none()
            }
            // the theme has been changed
            Message::ThemeChanged(theme) => {
                self.config.theme = theme;
                Command::none()
            }
            // the proxy mode has been changed
            Message::ProxyModeChanged(proxy_mode) => {
                if proxy_mode == ProxyMode::Single {
                    // if we're switching to single proxy mode, truncate the proxy list to 1
                    self.config.proxies.truncate(1);
                }

                self.config.proxy_mode = proxy_mode; // update the config
                self.rebuild_available = true; // there are changes that can be applied now
                Command::none()
            }
            // the proxy url has been changed
            Message::ProxyUrlChanged(value) => {
                if let Some(proxy_url) = self.config.proxies.get_mut(0) {
                    // update the proxy url
                    *proxy_url = value;
                } else {
                    // if there is no proxy url, add value to the proxy list
                    self.config.proxies.push(value);
                }

                self.rebuild_available = true; // there are changes that can be applied now

                Command::none()
            }
            // add proxies from file
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
                                }
                            }
                        }
                        Err(error) => {
                            self.error_modal = Some(format!("Failed to open file: {}", error));
                        }
                    };
                }

                Command::none()
            }
            // remove a proxy by index
            Message::RemoveProxy(index) => {
                self.config.proxies.remove(index); // remove the proxy
                self.rebuild_available = true; // there are changes that can be applied now
                Command::none()
            }
            // clear files that have been loaded for selection but not added to runner
            Message::ClearFiles => {
                self.files.clear(); // clear files
                self.file_filter.clear(); // clear file filter

                // get keys of loaded url inputs
                let keys: Vec<_> = self
                    .url_input
                    .data
                    .iter()
                    .filter(|(_, input)| input.status == UrlStatus::Loaded)
                    .map(|(index, _)| *index)
                    .collect();

                // remove inputs
                for key in keys {
                    self.url_input.remove(key);
                }

                // navigate to import if still on choose files
                if self.route == Route::ChooseFiles {
                    self.route = Route::Import;
                }

                Command::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Self::Message, Renderer<Self::Theme>> {
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
                                .align_items(Alignment::Center)
                                .push(Space::new(Length::Fixed(7_f32), Length::Shrink))
                                .push(
                                    text(download.node.name())
                                        .width(Length::Fill)
                                        .height(Length::Fill)
                                        .vertical_alignment(Vertical::Center),
                                )
                                .push(Space::new(Length::Fixed(3_f32), Length::Shrink))
                                .push(
                                    progress_bar(0_f32..=1_f32, progress)
                                        .style(theme::ProgressBar::Custom(Box::new(
                                            styles::progress_bar::ProgressBar,
                                        )))
                                        .width(Length::Fixed(80_f32))
                                        .height(Length::Fixed(15_f32)),
                                )
                                .push(Space::new(Length::Fixed(10_f32), Length::Shrink))
                                .push(
                                    text(
                                        format!("{} MB/s", pad_f32(download.speed()))
                                            .replace('0', "O"),
                                    )
                                    .width(Length::Shrink)
                                    .height(Length::Fill)
                                    .vertical_alignment(Vertical::Center)
                                    .font(Font::External {
                                        name: "Inconsolata",
                                        bytes: INCONSOLATA_MEDIUM,
                                    })
                                    .size(16),
                                )
                                .push(Space::new(Length::Fixed(5_f32), Length::Shrink))
                                .push(icon_button(X_ICON, Message::CancelDownload(id.clone())))
                                .push(pause_button)
                                .push(Space::new(Length::Fixed(7_f32), Length::Shrink)),
                        )
                        .style(theme::Container::Custom(Box::new(
                            styles::container::Download { index },
                        ))),
                    );
                }

                if self.active_downloads.is_empty() {
                    download_list = download_list.push(
                        text("No active downloads")
                            .height(Length::Fixed(30_f32))
                            .width(Length::Fixed(165_f32))
                            .vertical_alignment(Vertical::Center)
                            .horizontal_alignment(Horizontal::Center),
                    )
                }

                let mut download_group = Column::new().push(
                    scrollable(download_list)
                        .vertical_scroll(Properties::default().width(5).scroller_width(5).margin(0))
                        .style(theme::Scrollable::Custom(Box::new(
                            styles::scrollable::Scrollable,
                        )))
                        .height(Length::Fill),
                );

                if !self.active_downloads.is_empty() {
                    download_group = download_group.push(
                        Row::new()
                            .spacing(10)
                            .padding(8)
                            .height(Length::Fixed(45_f32))
                            .push(if self.all_paused {
                                button(" Resume All ")
                                    .on_press(Message::ResumeDownloads)
                                    .style(theme::Button::Custom(Box::new(styles::button::Button)))
                            } else {
                                button(" Pause All ")
                                    .on_press(Message::PauseDownloads)
                                    .style(theme::Button::Custom(Box::new(styles::button::Button)))
                            })
                            .push(
                                button(" Cancel All ")
                                    .on_press(Message::CancelDownloads)
                                    .style(theme::Button::Custom(Box::new(
                                        styles::button::WarningButton,
                                    ))),
                            )
                            .push(
                                container(
                                    text(
                                        format!(
                                            " {:.2} GB used ",
                                            self.bandwidth_counter as f32 / 1073741824_f32
                                        )
                                        .replace('0', "O"),
                                    )
                                    .font(Font::External {
                                        name: "Inconsolata",
                                        bytes: INCONSOLATA_MEDIUM,
                                    })
                                    .vertical_alignment(Vertical::Center)
                                    .height(Length::Fill),
                                )
                                .style(theme::Container::Custom(Box::new(styles::container::Pill)))
                                .height(Length::Fill),
                            ),
                    )
                }

                let mut error_log = Column::new().push(scrollable(self.error_log()).style(
                    theme::Scrollable::Custom(Box::new(styles::scrollable::Scrollable)),
                ));

                if self.errors.is_empty() {
                    error_log = error_log.push(
                        text("No errors")
                            .height(Length::Fixed(30_f32))
                            .width(Length::Fixed(70_f32))
                            .vertical_alignment(Vertical::Center)
                            .horizontal_alignment(Horizontal::Center),
                    )
                }

                container(
                    Column::new()
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .spacing(5)
                        .push(
                            container(download_group)
                                .style(theme::Container::Custom(Box::new(
                                    styles::container::DownloadList,
                                )))
                                .padding(2)
                                .width(Length::Fill)
                                .height(Length::FillPortion(2)),
                        )
                        .push(
                            container(error_log)
                                .style(theme::Container::Custom(Box::new(
                                    styles::container::DownloadList,
                                )))
                                .padding(8)
                                .width(Length::Fill)
                                .height(Length::FillPortion(1)),
                        ),
                )
            }
            Route::Import => container(
                Column::new()
                    .spacing(5)
                    .push(
                        scrollable(self.url_inputs())
                            .style(theme::Scrollable::Custom(Box::new(
                                styles::scrollable::Scrollable,
                            )))
                            .height(Length::Fill),
                    )
                    .push(
                        Row::new()
                            .spacing(10)
                            .push(
                                button(" Add from clipboard ")
                                    .style(theme::Button::Custom(Box::new(styles::button::Button)))
                                    .on_press(Message::AddUrlClipboard),
                            )
                            .push(
                                button(" + ")
                                    .style(theme::Button::Custom(Box::new(styles::button::Button)))
                                    .on_press(Message::AddInput),
                            )
                            .push(
                                button(" Load all ")
                                    .style(theme::Button::Custom(Box::new(styles::button::Button)))
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
                    .filter(|file| {
                        if let Some(download) = self.file_filter.get(file.node.hash()) {
                            *download && file.node.kind().is_file()
                        } else {
                            file.node.kind().is_file()
                        }
                    }) // only count files that are marked for download
                    .map(|file| file.node.size()) // get the size of every file
                    .sum(); // sum the sizes

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
                                        .style(theme::Button::Custom(Box::new(
                                            styles::button::Button,
                                        )))
                                        .on_press(Message::AddFiles),
                                )
                                .push(
                                    button(" Cancel ")
                                        .style(theme::Button::Custom(Box::new(
                                            styles::button::Button,
                                        )))
                                        .on_press(Message::ClearFiles),
                                )
                                .push(
                                    container(
                                        Text::new(
                                            format!(" {:.2} GB ", size as f32 / 1073741824_f32)
                                                .replace('0', "O"),
                                        )
                                        .font(Font::External {
                                            name: "Inconsolata",
                                            bytes: INCONSOLATA_MEDIUM,
                                        })
                                        .vertical_alignment(Vertical::Center)
                                        .horizontal_alignment(Horizontal::Center)
                                        .width(Length::Fill)
                                        .height(Length::Fill),
                                    )
                                    .style(theme::Container::Custom(Box::new(
                                        styles::container::Pill,
                                    )))
                                    .height(Length::Fill),
                                ),
                        ),
                )
            }
            Route::Settings => {
                let mut apply_button = button(" Apply ")
                    .style(theme::Button::Custom(Box::new(styles::button::Button)));

                if self.rebuild_available {
                    apply_button = apply_button.on_press(Message::RebuildMega);
                }

                // TODO max threads per should be divisible by max threads per file && max concurrent files

                container(
                    Column::new()
                        .width(Length::Fixed(350_f32))
                        .push(self.settings_slider(
                            0,
                            self.config.max_threads,
                            1_f64..=50_f64,
                            "Max threads:",
                        ))
                        .push(self.settings_slider(
                            1,
                            self.config.max_concurrent_files,
                            1_f64..=self.config.max_threads as f64,
                            "Max concurrent files:",
                        ))
                        .push(self.settings_slider(
                            2,
                            self.config.max_threads_per_file,
                            1_f64..=self.config.max_threads as f64,
                            "Max threads per files:",
                        ))
                        .push(self.settings_slider(
                            3,
                            self.config.timeout as usize,
                            100_f64..=60000_f64,
                            "Timeout:",
                        ))
                        .push(self.settings_slider(
                            4,
                            self.config.max_retries,
                            1_f64..=10_f64,
                            "Max retries:",
                        ))
                        .push(self.settings_slider(
                            5,
                            self.config.min_retry_delay as usize,
                            100_f64..=self.config.max_retry_delay as f64,
                            "Min Retry delay:",
                        ))
                        .push(self.settings_slider(
                            6,
                            self.config.max_retry_delay as usize,
                            self.config.min_retry_delay as f64..=60000_f64,
                            "Max Retry delay:",
                        ))
                        .push(Space::new(Length::Shrink, Length::Fixed(10_f32)))
                        .push(self.settings_picklist(
                            "Theme",
                            &Theme::ALL[..],
                            Some(self.config.theme),
                            Message::ThemeChanged,
                        ))
                        .push(Space::new(Length::Shrink, Length::Fixed(10_f32)))
                        .push(self.settings_picklist(
                            "Proxy Mode",
                            &ProxyMode::ALL[..],
                            Some(self.config.proxy_mode),
                            Message::ProxyModeChanged,
                        ))
                        .push(Space::new(Length::Shrink, Length::Fixed(10_f32)))
                        .push(self.proxy_selector())
                        .push(Space::new(Length::Shrink, Length::Fill))
                        .push(
                            Row::new()
                                .push(Space::new(Length::Fixed(8_f32), Length::Shrink))
                                .push(
                                    button(" Save ")
                                        .style(theme::Button::Custom(Box::new(
                                            styles::button::Button,
                                        )))
                                        .on_press(Message::SaveConfig),
                                )
                                .push(Space::new(Length::Fixed(10_f32), Length::Shrink))
                                .push(apply_button)
                                .push(Space::new(Length::Fixed(10_f32), Length::Shrink))
                                .push(
                                    button(" Reset ")
                                        .style(theme::Button::Custom(Box::new(
                                            styles::button::WarningButton,
                                        )))
                                        .on_press(Message::ResetConfig),
                                ),
                        ),
                )
            }
        };

        // nav + content = body
        let body = container(
            Row::new()
                .push(
                    container(
                        Column::new()
                            .padding(4)
                            .spacing(4)
                            .push(self.nav_button("Home", Route::Home, false))
                            .push(self.nav_button("Import", Route::Import, false))
                            .push(self.nav_button(
                                "Choose files",
                                Route::ChooseFiles,
                                self.files.is_empty(),
                            ))
                            .push(Space::new(Length::Shrink, Length::Fill))
                            .push(self.nav_button("Settings", Route::Settings, false)),
                    )
                    .width(Length::Fixed(170_f32))
                    .height(Length::Fill)
                    .style(theme::Container::Custom(Box::new(styles::container::Nav))),
                )
                .push(content.padding(10).width(Length::Fill)),
        )
        .style(theme::Container::Custom(Box::new(styles::container::Body)))
        .width(Length::Fill)
        .height(Length::Fill);

        if let Some(error_message) = &self.error_modal {
            container(Modal::new(
                body,
                container(
                    Column::new()
                        .spacing(5)
                        .push(
                            text(error_message)
                                .style(theme::Text::Color(Color::from_rgb8(255, 69, 0)))
                                .vertical_alignment(Vertical::Center)
                                .horizontal_alignment(Horizontal::Center),
                        )
                        .push(Space::new(Length::Fixed(100_f32), Length::Fixed(2_f32)))
                        .push(
                            Row::new()
                                .spacing(5)
                                .push(Space::new(Length::FillPortion(3), Length::Shrink))
                                .push(
                                    button(" Ok ")
                                        .style(theme::Button::Custom(Box::new(
                                            styles::button::Button,
                                        )))
                                        .on_press(Message::CloseModal), // .width(Length::FillPortion(1))
                                ),
                        ),
                )
                .width(Length::Fixed(150_f32))
                .padding(10)
                .style(theme::Container::Custom(Box::new(styles::container::Modal))),
            ))
            .into()
        } else {
            body.into()
        }
    }

    // determines the correct Theme based on configuration & system settings
    fn theme(&self) -> Self::Theme {
        match self.config.theme {
            Theme::System => match dark_light::detect() {
                dark_light::Mode::Light => iced::Theme::Light,
                _ => iced::Theme::Dark, // Default and Dark map to Dark
            },
            Theme::Light => iced::Theme::Light,
            Theme::Dark => iced::Theme::Dark,
        }
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // reads runner messages from channel and sends them to the UI
        let runner_subscription = iced::subscription::unfold(
            "runner messages",
            self.runner_receiver.take(),
            move |mut receiver| async move {
                let message = receiver.as_mut().unwrap().recv().await.unwrap();
                (Message::Runner(message), receiver)
            },
        );

        // reads mega messages from channel and sends them to the UI
        let mega_subscription = iced::subscription::unfold(
            "mega messages",
            self.mega_receiver.take(),
            move |mut receiver| async move {
                let message = receiver.as_mut().unwrap().recv().await.unwrap();
                (Message::Mega(message.into()), receiver)
            },
        );

        // forces the UI to refresh every second
        // this is needed because changes to the active downloads dont trigger a refresh
        let refresh = every(Duration::from_secs(1)).map(|_| Message::Refresh);

        // run all subscriptions in parallel
        Subscription::batch(vec![runner_subscription, mega_subscription, refresh])
    }
}

// initializes the app from the config
impl From<Config> for App {
    fn from(config: Config) -> Self {
        // create a channel for the mega client to send messages to the UI
        let (tx, mega_receiver) = unbounded_channel();
        let mega_sender = Arc::new(tx);

        // build the mega client
        let mega = mega_builder(&config, Arc::clone(&mega_sender)).unwrap();

        // create a channel for the runner to send messages to the UI
        let (tx, runner_receiver) = unbounded_channel();
        let runner_sender = Arc::new(tx);

        Self {
            config,
            mega,
            runner: None,
            download_queue: DownloadQueue::default(),
            queued: Arc::new(Default::default()),
            active_downloads: HashMap::new(),
            runner_sender,
            runner_receiver: RefCell::new(Some(runner_receiver)),
            mega_sender,
            mega_receiver: RefCell::new(Some(mega_receiver)),
            files: Vec::new(),
            file_filter: HashMap::new(),
            url_input: IndexMap::default(),
            expanded_files: HashMap::new(),
            route: Route::Home,
            url_regex: Regex::new("https?://mega\\.nz/folder/([\\dA-Za-z]+)#([\\dA-Za-z]+)").unwrap(),
            proxy_regex: Regex::new("(?:(?:https?|socks5h?)://)(?:(?:[a-z\\d]+(?::[a-z\\d]+)?@)?)(?:(?:[a-z\\d](?:[a-z\\d\\-]{0,61}[a-z\\d])?\\.)+[a-z\\d][a-z\\d\\-]{0,61}[a-z\\d]|(?:\\d{1,3}\\.){3}\\d{1,3})(:\\d{1,5})").unwrap(),
            errors: Vec::new(),
            error_modal: None,
            all_paused: false,
            bandwidth_counter: 0,
            rebuild_available: false,
            active_threads: Default::default(),
        }
    }
}

impl App {
    fn recursive_files<'a>(&self, file: &'a MegaFile) -> Element<'a, Message> {
        if file.children.is_empty() {
            Row::new()
                .spacing(5)
                .push(
                    text(file.node.name())
                        .width(Length::Fill)
                        .vertical_alignment(Vertical::Center),
                )
                .push(
                    checkbox(
                        "",
                        *self.file_filter.get(file.node.hash()).unwrap_or(&true),
                        |value| Message::ToggleFile((value, file.clone())),
                    )
                    .style(theme::Checkbox::Custom(Box::new(
                        styles::checkbox::Checkbox,
                    ))),
                )
                .into()
        } else {
            let expanded = *self.expanded_files.get(file.node.hash()).unwrap_or(&false);

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
                        .style(theme::Button::Custom(Box::new(styles::button::IconButton)))
                        .on_press(Message::ToggleExpanded(file.node.hash().to_string()))
                        .padding(3),
                    )
                    .push(
                        text(file.node.name())
                            .width(Length::Fill)
                            .vertical_alignment(Vertical::Center),
                    )
                    .push(
                        checkbox(
                            "",
                            *self.file_filter.get(file.node.hash()).unwrap_or(&true),
                            |value| Message::ToggleFile((value, file.clone())),
                        )
                        .style(theme::Checkbox::Custom(Box::new(
                            styles::checkbox::Checkbox,
                        ))),
                    ),
            );

            if expanded {
                for file in &file.children {
                    column = column.push(
                        Row::new()
                            .push(Space::new(Length::Fixed(20.0), Length::Shrink))
                            .push(self.recursive_files(file)),
                    );
                }
            }

            column.into()
        }
    }

    fn nav_button<'a>(&self, label: &'a str, route: Route, disabled: bool) -> Element<'a, Message> {
        let style = if disabled {
            styles::svg::SvgIcon::new(Color::from_rgb8(235, 28, 48).into())
        } else {
            styles::svg::SvgIcon::new(Color::from_rgb8(255, 48, 78).into())
        };

        let mut row = Row::new()
            .align_items(Alignment::Center)
            .height(Length::Fixed(40_f32));

        if self.route == route {
            row = row
                .push(
                    svg(svg::Handle::from_memory(SELECTED_ICON))
                        .style(theme::Svg::Custom(Box::new(style.clone())))
                        .width(Length::Fixed(4_f32))
                        .height(Length::Fixed(25_f32)),
                )
                .push(Space::new(Length::Fixed(8_f32), Length::Shrink))
        } else {
            row = row.push(Space::new(Length::Fixed(12_f32), Length::Shrink))
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
                        .style(theme::Svg::Custom(Box::new(style))),
                )
                .padding(4)
                .style(theme::Container::Custom(Box::new(
                    styles::container::Icon::new(self.route == route),
                ))),
            )
            .push(Space::new(Length::Fixed(12_f32), Length::Shrink));

        let mut button = button(row.push(text(label)))
            .style(theme::Button::Custom(Box::new(styles::button::Nav {
                active: self.route == route,
            })))
            .width(Length::Fill)
            .padding(0);

        if !disabled {
            button = button.on_press(Message::Navigate(route));
        }

        button.into()
    }

    fn error_log(&self) -> Element<Message> {
        let mut column = Column::new().spacing(2).width(Length::Fill);

        for error in &self.errors {
            column =
                column.push(text(error).style(theme::Text::Color(Color::from_rgb8(255, 69, 0))));
        }

        column.into()
    }

    fn url_inputs(&self) -> Element<Message> {
        let mut inputs = Column::new().spacing(5);

        for (index, input) in self.url_input.data.iter() {
            let mut text_input = text_input("Url", &input.value)
                .style(theme::TextInput::Custom(Box::new(
                    styles::text_input::UrlInput { mode: input.status },
                )))
                .size(18)
                .padding(8);

            if input.status == UrlStatus::Invalid || input.status == UrlStatus::None {
                text_input = text_input
                    .on_input(|value| Message::UrlInput((*index, value)))
                    .on_submit(Message::AddUrl(*index));
            }

            let mut row = Row::new()
                .spacing(5)
                .align_items(Alignment::Center)
                .push(text_input);

            match input.status {
                UrlStatus::None | UrlStatus::Invalid => {
                    row = row.push(
                        button(
                            svg(svg::Handle::from_memory(TRASH_ICON))
                                .width(Length::Fixed(22_f32))
                                .height(Length::Fixed(22_f32)),
                        )
                        .style(theme::Button::Custom(Box::new(styles::button::IconButton)))
                        .on_press(Message::RemoveInput(*index))
                        .padding(4),
                    );
                }
                UrlStatus::Loading(angle) => {
                    row = row.push(
                        canvas(LoadingWheel::new(angle))
                            .width(Length::Fixed(30_f32))
                            .height(Length::Fixed(30_f32)),
                    );
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
            .push(Space::new(Length::Fixed(8_f32), Length::Shrink))
            .push(
                text(label)
                    .vertical_alignment(Vertical::Center)
                    .height(Length::Fill),
            )
            .push(Space::new(Length::Fill, Length::Shrink))
            .push(
                text(pad_usize(value).replace('0', "O"))
                    .font(Font::External {
                        name: "Inconsolata",
                        bytes: INCONSOLATA_MEDIUM,
                    })
                    .vertical_alignment(Vertical::Center)
                    .height(Length::Fill),
            )
            .push(Space::new(Length::Fixed(10_f32), Length::Shrink))
            .push(
                Slider::new(range, value as f64, move |value| {
                    Message::SettingsSlider((index, value))
                })
                .width(Length::Fixed(130_f32))
                .height(30)
                .style(theme::Slider::Custom(Box::new(styles::slider::Slider))),
            )
            .into()
    }

    fn settings_picklist<'a, T>(
        &self,
        label: &'a str,
        options: impl Into<Cow<'a, [T]>>,
        selected: Option<T>,
        message: fn(T) -> Message,
    ) -> Element<'a, Message>
    where
        T: ToString + Eq + 'static + Clone,
        [T]: ToOwned<Owned = Vec<T>>,
    {
        Row::new()
            .height(Length::Fixed(30_f32))
            .push(Space::new(Length::Fixed(8_f32), Length::Shrink))
            .push(
                text(label)
                    .vertical_alignment(Vertical::Center)
                    .height(Length::Fill),
            )
            .push(Space::new(Length::Fill, Length::Shrink))
            .push(
                pick_list(options, selected, message)
                    .width(Length::Fixed(170_f32))
                    .style(theme::PickList::Custom(
                        Rc::new(styles::pick_list::PickList),
                        Rc::new(styles::menu::Menu),
                    )),
            )
            .into()
    }

    fn proxy_selector(&self) -> Element<Message> {
        let mut column = Column::new();

        if self.config.proxy_mode == ProxyMode::Random {
            let mut proxy_display = Column::new().width(Length::Fill);

            for (index, proxy) in self.config.proxies.iter().enumerate() {
                proxy_display = proxy_display.push(
                    container(
                        Row::new()
                            .padding(4)
                            .push(text(proxy))
                            .push(Space::new(Length::Fill, Length::Shrink))
                            .push(
                                button(
                                    svg(svg::Handle::from_memory(X_ICON))
                                        .width(Length::Fixed(15_f32))
                                        .height(Length::Fixed(15_f32)),
                                )
                                .style(theme::Button::Custom(Box::new(styles::button::IconButton)))
                                .on_press(Message::RemoveProxy(index))
                                .padding(4),
                            )
                            .push(Space::new(Length::Fixed(8_f32), Length::Shrink)),
                    )
                    .style(theme::Container::Custom(Box::new(
                        styles::container::Download { index },
                    )))
                    .width(Length::Fill),
                )
            }

            if self.config.proxies.is_empty() {
                proxy_display = proxy_display.push(
                    text("No proxies")
                        .width(Length::Fixed(100_f32))
                        .height(Length::Fixed(35_f32))
                        .vertical_alignment(Vertical::Center)
                        .horizontal_alignment(Horizontal::Center),
                );
            }

            column = column.push(
                container(
                    Column::new()
                        .push(scrollable(proxy_display).height(Length::Fixed(125_f32)))
                        .push(Space::new(Length::Shrink, Length::Fill))
                        .push(
                            container(
                                button(" Add proxies ")
                                    .on_press(Message::AddProxies)
                                    .style(theme::Button::Custom(Box::new(styles::button::Button)))
                                    .padding(4),
                            )
                            .padding(5),
                        ),
                )
                .style(theme::Container::Custom(Box::new(
                    styles::container::DownloadList,
                )))
                .height(Length::Fixed(170_f32))
                .padding(2),
            );
        } else if self.config.proxy_mode == ProxyMode::Single {
            column = column.push(
                text_input(
                    "Proxy url",
                    self.config.proxies.get(0).unwrap_or(&String::new()),
                )
                .on_input(Message::ProxyUrlChanged)
                .style(theme::TextInput::Custom(Box::new(
                    styles::text_input::UrlInput {
                        mode: UrlStatus::None,
                    },
                )))
                .padding(6),
            );
        }

        Row::new()
            .push(Space::new(Length::Fixed(8_f32), Length::Shrink))
            .push(column)
            .into()
    }

    fn start_runner(&self, workers: usize) -> Vec<JoinHandle<()>> {
        runner(
            self.config.clone(),
            &self.mega,
            &self.download_queue,
            &self.runner_sender,
            &self.queued,
            &self.active_threads,
            workers,
        )
    }

    fn stop_runner(&mut self) {
        if let Some(runner_threads) = self.runner.take() {
            for thread in runner_threads {
                thread.abort();
            }
        }
    }
}

// a wrapper around HashMap that uses an incrementing index as the key
struct IndexMap<T> {
    data: HashMap<usize, T>,
    unused_indices: Vec<usize>,
    next_index: usize,
}

impl<T> IndexMap<T>
where
    T: Default,
{
    fn default() -> Self {
        Self {
            data: HashMap::from([(0, T::default())]),
            unused_indices: Vec::new(),
            next_index: 1,
        }
    }

    fn insert(&mut self, value: T) -> usize {
        let index = if let Some(unused_index) = self.unused_indices.pop() {
            unused_index
        } else {
            let index = self.next_index;
            self.next_index += 1;
            index
        };

        self.data.insert(index, value);
        index
    }

    fn update(&mut self, index: usize, value: T) -> Option<T> {
        self.data.insert(index, value)
    }

    fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.data.get_mut(&index)
    }

    fn remove(&mut self, index: usize) -> Option<T> {
        let value = self.data.remove(&index);

        if value.is_some() {
            self.unused_indices.push(index);
        }

        value
    }
}

// GUI settings
pub(crate) fn settings() -> Settings<Config> {
    // TODO let icon = load_from_memory(ICON).unwrap();

    Settings {
        id: None,
        window: Window {
            size: (700, 550),
            position: Default::default(),
            min_size: Some((550, 500)),
            max_size: None,
            visible: true,
            resizable: true,
            decorations: true,
            transparent: false,
            always_on_top: false,
            icon: None, // Some(from_rgba(icon.to_rgba8().into_raw(), 32, 32).unwrap()),
            #[cfg(target_os = "macos")]
            platform_specific: PlatformSpecific::default(),
            #[cfg(target_os = "windows")]
            platform_specific: PlatformSpecific::default(),
            #[cfg(target_os = "linux")]
            platform_specific: PlatformSpecific,
        },
        flags: Config::load().expect("failed to load config"), // load config
        default_font: Some(CABIN_REGULAR),
        default_text_size: 20.0,
        exit_on_close_request: true,
        antialiasing: true,
        text_multithreading: true,
        try_opengles_first: false,
    }
}

// build a new mega client from config
fn mega_builder(
    config: &Config,
    sender: Arc<UnboundedSender<mega::Error>>,
) -> mega::Result<Client> {
    if config.proxy_mode != ProxyMode::None && config.proxies.is_empty() {
        Err(mega::Error::NoProxies)
    } else {
        // build http client
        let http_client = HttpClient::builder()
            .proxy(Proxy::custom({
                let proxies = config.proxies.clone();
                let proxy_mode = config.proxy_mode;

                move |_| match proxy_mode {
                    ProxyMode::Random => {
                        let i = fastrand::usize(..proxies.len());
                        let proxy_url = &proxies[i];
                        Url::parse(proxy_url).unwrap().into()
                    }
                    ProxyMode::Single => {
                        let proxy_url = &proxies[0];
                        Url::parse(proxy_url).unwrap().into()
                    }
                    ProxyMode::None => None::<Url>,
                }
            }))
            .timeout(Duration::from_millis(config.timeout))
            .build()
            .unwrap();

        // build mega client
        Client::builder()
            .https(false)
            .timeout(Duration::from_millis(config.timeout))
            .max_retry_delay(Duration::from_millis(config.max_retry_delay))
            .min_retry_delay(Duration::from_millis(config.min_retry_delay))
            .max_retries(config.max_retries)
            .sender(sender)
            .build(http_client)
    }
}

// build an icon button
fn icon_button(icon: &'static [u8], message: Message) -> Element<Message> {
    button(
        svg(svg::Handle::from_memory(icon))
            .height(Length::Fixed(25_f32))
            .width(Length::Fixed(25_f32)),
    )
    .padding(4)
    .style(theme::Button::Custom(Box::new(styles::button::IconButton)))
    .on_press(message)
    .into()
}

// pads a usize with spaces
fn pad_usize(num: usize) -> String {
    let mut s = num.to_string();

    while s.len() < 3 {
        s.push(' ');
    }

    s
}

// rounds f32 & pads with spaces
fn pad_f32(num: f32) -> String {
    let mut s = if num < 0.0001 {
        String::from("0")
    } else if num < 10.0 {
        format!("{:.2}", num)
    } else if num < 100.0 {
        format!("{:.1}", num)
    } else {
        format!("{:.0}", num)
    };

    while s.len() <= 3 {
        s.insert(0, ' ');
    }

    s
}
