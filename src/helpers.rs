#[cfg(feature = "gui")]
use crate::RunnerMessage;
#[cfg(feature = "gui")]
use crate::WorkerHandle;
use crate::config::Config;
use crate::mega_client::MegaClient;
#[cfg(feature = "gui")]
use crate::screens::{ChooseFilesMessage, HomeMessage, ImportMessage, SettingsMessage};
use crate::{ProxyMode, styles};
#[cfg(feature = "gui")]
use iced::futures::Stream;
#[cfg(feature = "gui")]
use iced::futures::sink::SinkExt;
#[cfg(feature = "gui")]
use iced::widget::svg::{Status, Style};
#[cfg(feature = "gui")]
use iced::widget::{button, svg};
#[cfg(feature = "gui")]
use iced::{Element, Length, Theme, stream};
use reqwest::{Client, Proxy};
#[cfg(feature = "gui")]
use std::collections::HashMap;
use std::time::Duration;
#[cfg(feature = "gui")]
use tokio::sync::mpsc::{Sender, channel};
use tokio::time::{MissedTickBehavior, interval};
#[cfg(feature = "gui")]
use tokio_util::sync::CancellationToken;
use url::Url;

#[cfg(feature = "gui")]
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// force the GUI to update
    Refresh,
    /// home screen message
    Home(HomeMessage),
    /// import screen message
    Import(ImportMessage),
    /// choose files screen message
    ChooseFiles(ChooseFilesMessage),
    /// received batch of messages from runner
    RunnerBatch(Vec<RunnerMessage>),
    /// runner subscription is ready, provides sender for workers
    RunnerReady(Sender<RunnerMessage>),
    /// navigate to a different route
    Navigate(Route),
    /// close the error modal
    CloseModal,
    /// settings screen message
    Settings(SettingsMessage),
    /// remove any loaded files
    ClearFiles,
}

#[cfg(feature = "gui")]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Route {
    Home,
    Import,
    ChooseFiles,
    Settings,
}

#[cfg(feature = "gui")]
#[derive(Default)]
pub(crate) struct UrlInput {
    pub(crate) value: String,
    pub(crate) status: UrlStatus,
}

#[cfg(feature = "gui")]
#[derive(PartialEq, Clone, Copy, Default)]
pub(crate) enum UrlStatus {
    #[default]
    None,
    Invalid,
    Loading,
    Loaded,
}

/// a wrapper around HashMap that uses an incrementing index as the key
#[cfg(feature = "gui")]
pub(crate) struct IndexMap<T> {
    pub(crate) data: HashMap<usize, T>,
    unused_indices: Vec<usize>,
    next_index: usize,
}

#[cfg(feature = "gui")]
impl<T: Default> Default for IndexMap<T> {
    fn default() -> Self {
        Self {
            data: HashMap::from([(0, T::default())]),
            unused_indices: Vec::new(),
            next_index: 1,
        }
    }
}

#[cfg(feature = "gui")]
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
        let existed = self.data.contains_key(&index);
        let prev = self.data.insert(index, value);

        // If we inserted a new key manually, keep the internal bookkeeping consistent:
        // - avoid handing out this index again via `insert` (remove from unused_indices)
        // - avoid reusing `next_index` if caller inserted at/above it
        if !existed {
            if let Some(pos) = self.unused_indices.iter().position(|&i| i == index) {
                self.unused_indices.swap_remove(pos);
            }

            if index >= self.next_index {
                self.next_index = index + 1;
            }
        }

        prev
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

#[cfg(feature = "gui")]
pub(crate) struct WorkerState {
    pub(crate) handles: Vec<WorkerHandle>,
    pub(crate) cancel: CancellationToken,
}

/// build a new mega client from config
pub(crate) fn mega_builder(config: &Config) -> anyhow::Result<MegaClient> {
    config
        .validate()
        .map_err(|message| anyhow::anyhow!("invalid config: {message}"))?;

    // build http client
    let http_client = Client::builder()
        .proxy(Proxy::custom({
            let proxies = config.proxies.clone();
            let proxy_mode = config.proxy_mode;

            move |_| match proxy_mode {
                ProxyMode::Random => {
                    let i = fastrand::usize(..proxies.len());
                    Some(proxies[i].clone())
                }
                ProxyMode::Single => Some(proxies[0].clone()),
                ProxyMode::None => None::<Url>,
            }
        }))
        .connect_timeout(config.timeout)
        .read_timeout(config.timeout)
        .tcp_keepalive(None)
        .build()?;

    MegaClient::new(http_client)
}

#[cfg(feature = "gui")]
pub(crate) fn runner_worker() -> impl Stream<Item = Message> {
    stream::channel(100, async |mut output| {
        // Create tokio channel for workers
        let (sender, mut receiver) = channel::<RunnerMessage>(64);

        // Send the sender back to the application
        if output.send(Message::RunnerReady(sender)).await.is_err() {
            return;
        }

        let mut batch = Vec::new();
        let mut flush_interval = interval(Duration::from_millis(100));
        _ = flush_interval.tick();
        flush_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                maybe_msg = receiver.recv() => {
                    let Some(msg) = maybe_msg else {
                        batch.push(RunnerMessage::Finished);
                        _ = output.send(Message::RunnerBatch(std::mem::take(&mut batch))).await;
                        break;
                    };

                    let is_finished = matches!(msg, RunnerMessage::Finished);
                    batch.push(msg);
                    if is_finished {
                        _ = output.send(Message::RunnerBatch(std::mem::take(&mut batch))).await;
                        break;
                    }
                }
                _ = flush_interval.tick() => {
                    if !batch.is_empty()
                        && output.send(Message::RunnerBatch(std::mem::take(&mut batch))).await.is_err()
                    {
                        break;
                    }
                }
            }
        }
    })
}

/// build an icon button
#[cfg(feature = "gui")]
pub(crate) fn icon_button<M: Clone + 'static>(
    icon: &'static [u8],
    message: M,
    style: impl Fn(&Theme, Status) -> Style + 'static,
) -> Element<'static, M> {
    button(
        svg(svg::Handle::from_memory(icon))
            .height(Length::Fixed(25_f32))
            .width(Length::Fixed(25_f32))
            .style(style),
    )
    .padding(4)
    .style(styles::button::icon)
    .on_press(message)
    .into()
}

/// pads a usize with spaces
#[cfg(feature = "gui")]
pub(crate) fn pad_usize(num: usize) -> String {
    let mut s = num.to_string();

    while s.len() < 3 {
        s.push(' ');
    }

    s
}

/// Format a byte count into a human-readable string
pub(crate) fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{b} B")
    }
}

/// rounds f32 & pads with spaces
#[cfg(feature = "gui")]
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
