use crate::config::Config;
use crate::mega_client::MegaClient;
use crate::{MegaFile, ProxyMode, RunnerMessage, WorkerHandle};
use iced::futures::Stream;
use iced::futures::sink::SinkExt;
use iced::widget::svg::{Status, Style};
use iced::widget::{button, svg};
use iced::{Element, Length, Theme, stream};
use reqwest::{Client, Proxy};
use std::collections::HashMap;
use tokio::sync::mpsc::{Sender, channel};
use tokio_util::sync::CancellationToken;
use url::Url;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// force the GUI to update
    Refresh,
    /// add url from clipboard
    AddUrlClipboard,
    /// got clipboard contents
    GotClipboard(Option<String>),
    /// url added by user
    AddUrl(usize),
    /// add all the urls
    AddAllUrls,
    /// backend got files for url
    GotFiles(Result<(Vec<MegaFile>, usize), usize>),
    /// user added files to download queue
    AddFiles,
    /// received message from runner
    Runner(RunnerMessage),
    /// runner subscription is ready, provides sender for workers
    RunnerReady(Sender<RunnerMessage>),
    /// navigate to a different route
    Navigate(Route),
    /// toggle file & children for download
    ToggleFile(Box<(bool, MegaFile)>),
    /// when a character is changed in the url input
    UrlInput((usize, String)),
    /// toggle expanded state of file tree
    ToggleExpanded(String),
    /// create a new url input
    AddInput,
    /// remove a url input
    RemoveInput(usize),
    /// close the error modal
    CloseModal,
    /// cancel all downloads
    CancelDownloads,
    /// cancel download by id
    CancelDownload(String),
    /// pause all downloads
    PauseDownloads,
    /// pause download by id
    PauseDownload(String),
    /// resume all downloads
    ResumeDownloads,
    /// resume download by id
    ResumeDownload(String),
    /// rebuild mega client with new config
    RebuildMega,
    /// when a settings slider is changed, usize is index
    SettingsSlider((usize, f64)),
    /// save current config to disk
    SaveConfig,
    /// reset config to default
    ResetConfig,
    /// theme changed
    ThemeChanged(Theme),
    /// proxy mode changed
    ProxyModeChanged(ProxyMode),
    /// proxy url changed, single proxy mode
    ProxyUrlChanged(String),
    /// add proxies from file
    AddProxies,
    /// remove proxy
    RemoveProxy(usize),
    /// remove any loaded files
    ClearFiles,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Route {
    Home,
    Import,
    ChooseFiles,
    Settings,
}

#[derive(Default)]
pub(crate) struct UrlInput {
    pub(crate) value: String,
    pub(crate) status: UrlStatus,
}

#[derive(PartialEq, Clone, Copy, Default)]
pub(crate) enum UrlStatus {
    #[default]
    None,
    Invalid,
    Loading,
    Loaded,
}

/// a wrapper around HashMap that uses an incrementing index as the key
pub(crate) struct IndexMap<T> {
    pub(crate) data: HashMap<usize, T>,
    unused_indices: Vec<usize>,
    next_index: usize,
}

impl<T: Default> Default for IndexMap<T> {
    fn default() -> Self {
        Self {
            data: HashMap::from([(0, T::default())]),
            unused_indices: Vec::new(),
            next_index: 1,
        }
    }
}

impl<T> IndexMap<T>
where
    T: Default,
{
    pub(crate) fn insert(&mut self, value: T) -> usize {
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

    pub(crate) fn update(&mut self, index: usize, value: T) -> Option<T> {
        self.data.insert(index, value)
    }

    pub(crate) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.data.get_mut(&index)
    }

    pub(crate) fn remove(&mut self, index: usize) -> Option<T> {
        let value = self.data.remove(&index);

        if value.is_some() {
            self.unused_indices.push(index);
        }

        value
    }
}

pub(crate) struct WorkerState {
    pub(crate) handles: Vec<WorkerHandle>,
    pub(crate) cancel: CancellationToken,
}

/// build a new mega client from config
pub(crate) fn mega_builder(config: &Config) -> anyhow::Result<MegaClient> {
    if config.proxy_mode != ProxyMode::None && config.proxies.is_empty() {
        Err(anyhow::Error::msg("no proxies"))
    } else {
        // build http client
        let http_client = Client::builder()
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
            .connect_timeout(config.timeout)
            .read_timeout(config.timeout)
            .tcp_keepalive(None)
            .build()?;

        MegaClient::new(http_client)
    }
}

pub(crate) fn runner_worker() -> impl Stream<Item = Message> {
    stream::channel(100, async |mut output| {
        // Create tokio channel for workers
        let (sender, mut receiver) = channel::<RunnerMessage>(64);

        // Send the sender back to the application
        if output.send(Message::RunnerReady(sender)).await.is_err() {
            return;
        }

        loop {
            // Read next message from workers
            let msg = if let Some(msg) = receiver.recv().await {
                msg
            } else {
                RunnerMessage::Finished
            };

            // Forward message to UI
            let is_finished = matches!(msg, RunnerMessage::Finished)
                | output.send(Message::Runner(msg)).await.is_err();

            if is_finished {
                break;
            }
        }
    })
}

/// build an icon button
pub(crate) fn icon_button(
    icon: &'static [u8],
    message: Message,
    style: impl Fn(&Theme, Status) -> Style + 'static,
) -> Element<'static, Message> {
    button(
        svg(svg::Handle::from_memory(icon))
            .height(Length::Fixed(25_f32))
            .width(Length::Fixed(25_f32))
            .style(style),
    )
    .padding(4)
    .style(button::background)
    .on_press(message)
    .into()
}

/// pads a usize with spaces
pub(crate) fn pad_usize(num: usize) -> String {
    let mut s = num.to_string();

    while s.len() < 3 {
        s.push(' ');
    }

    s
}

/// rounds f32 & pads with spaces
pub(crate) fn pad_f32(num: f32) -> String {
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
