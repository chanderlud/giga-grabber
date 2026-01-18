use crate::ProxyMode;
use crate::app::MONOSPACE;
use crate::config::{Config, MAX_CONCURRENCY, MAX_MAX_WORKERS, MIN_CONCURRENCY, MIN_MAX_WORKERS};
use crate::helpers::{UrlStatus, pad_usize};
use crate::resources::X_ICON;
use crate::styles;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    Column, Row, button, container, pick_list, scrollable, slider, space, svg, text, text_input,
};
use iced::{Element, Length, Theme};
use native_dialog::FileDialogBuilder;
use num_traits::cast::ToPrimitive;
use std::borrow::Cow;
use std::io::Read;
use std::ops::RangeInclusive;
use std::time::Duration;
use url::Url;

#[derive(Clone)]
pub(crate) struct Settings {
    pub(crate) config: Config,
    pub(crate) rebuild_available: bool,
    proxy_input: String,
    theme_options: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    SettingsSlider((usize, f64)),
    SaveConfig,
    ResetConfig,
    ThemeChanged(String),
    ProxyModeChanged(ProxyMode),
    ProxyUrlChanged(String),
    AddProxies,
    RemoveProxy(usize),
    RebuildMega,
}

pub(crate) enum Action {
    None,
    ConfigSaved,
    RebuildRequired(Config),
    ShowError(String),
}

impl Settings {
    pub(crate) fn new(config: Config) -> Self {
        let mut themes = vec!["Vanilla".to_string(), "System".to_string()];
        themes.extend(Theme::ALL.iter().map(|t| t.to_string()));

        let mut settings = Self {
            rebuild_available: false,
            config,
            proxy_input: String::new(),
            theme_options: themes,
        };

        // Preserve single-proxy input state when reconstructing Settings from an existing config.
        if settings.config.proxy_mode == ProxyMode::Single && !settings.config.proxies.is_empty() {
            settings.proxy_input = settings.config.proxies[0].to_string();
        }

        settings
    }

    pub(crate) fn set_rebuild_available(&mut self, flag: bool) {
        self.rebuild_available = flag;
    }

    pub(crate) fn update(&mut self, message: Message) -> Action {
        // handle proxy input validation in one place
        if matches!(message, Message::SaveConfig | Message::RebuildMega)
            && self.config.proxy_mode == ProxyMode::Single
        {
            let config_proxy = self.config.proxies.first().map(|p| p.to_string());
            if self.config.proxies.is_empty()
                || config_proxy.as_deref() != Some(self.proxy_input.as_str())
            {
                if let Ok(proxy) = Url::parse(&self.proxy_input) {
                    self.config.proxies = vec![proxy];
                } else {
                    return Action::ShowError("Invalid proxy URL".to_string());
                }
            }
        }

        match message {
            Message::SettingsSlider((index, value)) => {
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

                self.rebuild_available = true;
                Action::None
            }
            Message::SaveConfig => {
                self.config.normalize();
                if let Err(message) = self.config.validate() {
                    Action::ShowError(message)
                } else if let Err(error) = self.config.save() {
                    Action::ShowError(format!("Failed to save configuration: {}", error))
                } else {
                    Action::ConfigSaved
                }
            }
            Message::ResetConfig => {
                self.config = Config::default();
                self.rebuild_available = true;
                Action::None
            }
            Message::ThemeChanged(theme) => {
                self.config.theme = theme;
                Action::None
            }
            Message::ProxyModeChanged(proxy_mode) => {
                if proxy_mode == ProxyMode::Single {
                    self.config.proxies.truncate(1);
                    if let Some(proxy) = self.config.proxies.first() {
                        self.proxy_input = proxy.to_string();
                    }
                } else if proxy_mode == ProxyMode::None {
                    self.config.proxies.clear();
                    self.proxy_input.clear();
                }
                self.config.proxy_mode = proxy_mode;
                self.rebuild_available = true;
                Action::None
            }
            Message::ProxyUrlChanged(value) => {
                self.proxy_input = value;
                self.rebuild_available = true;
                Action::None
            }
            Message::AddProxies => {
                if let Some(file_path) = FileDialogBuilder::default()
                    .add_filter("Text File", ["txt"])
                    .open_single_file()
                    .location
                {
                    match std::fs::File::open(file_path) {
                        Ok(mut file) => {
                            let mut contents = String::new();
                            file.read_to_string(&mut contents).unwrap();

                            let mut any_added = false;
                            for line in contents.lines() {
                                if let Ok(url) = Url::parse(line) {
                                    self.config.proxies.push(url);
                                    any_added = true;
                                }
                            }

                            if any_added {
                                self.rebuild_available = true;
                            }
                        }
                        Err(error) => {
                            return Action::ShowError(format!("Failed to open file: {}", error));
                        }
                    }
                }

                Action::None
            }
            Message::RemoveProxy(index) => {
                self.config.proxies.remove(index);
                self.rebuild_available = true;
                Action::None
            }
            Message::RebuildMega => {
                self.config.normalize();
                if let Err(message) = self.config.validate() {
                    Action::ShowError(message)
                } else {
                    Action::RebuildRequired(self.config.clone())
                }
            }
        }
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let mut apply_button = button(" Apply ").style(styles::button::primary);

        if self.rebuild_available {
            apply_button = apply_button.on_press(Message::RebuildMega);
        }

        container(
            Column::new()
                .width(Length::Fixed(350_f32))
                .push(self.settings_slider(
                    0,
                    self.config.max_workers,
                    MIN_MAX_WORKERS as f64..=MAX_MAX_WORKERS as f64,
                    "Max Workers:",
                ))
                .push(self.settings_slider(
                    1,
                    self.config.concurrency_budget,
                    MIN_CONCURRENCY as f64..=MAX_CONCURRENCY as f64,
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
                                self.theme_options.clone(),
                                Some(self.config.theme.clone()),
                                Message::ThemeChanged,
                            )
                            .width(Length::Fixed(170_f32))
                            .style(styles::pick_list::default),
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
                                .style(styles::button::primary)
                                .on_press(Message::SaveConfig),
                        )
                        .push(space::horizontal().width(Length::Fixed(10_f32)))
                        .push(apply_button)
                        .push(space::horizontal().width(Length::Fixed(10_f32)))
                        .push(
                            button(" Reset ")
                                .style(styles::button::warning)
                                .on_press(Message::ResetConfig),
                        ),
                ),
        )
        .into()
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
        T: ToString + Eq + Clone + 'static,
        [T]: ToOwned<Owned = Vec<T>>,
    {
        Row::new()
            .height(Length::Fixed(30_f32))
            .push(space::horizontal().width(Length::Fixed(8_f32)))
            .push(text(label).align_y(Vertical::Center).height(Length::Fill))
            .push(space::horizontal())
            .push(
                pick_list(options, selected, message)
                    .width(Length::Fixed(170_f32))
                    .style(styles::pick_list::default),
            )
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
                            .push(text(proxy.to_string()))
                            .push(space::horizontal())
                            .push(
                                button(
                                    svg(svg::Handle::from_memory(X_ICON))
                                        .width(Length::Fixed(15_f32))
                                        .height(Length::Fixed(15_f32)),
                                )
                                .style(styles::button::icon)
                                .on_press(Message::RemoveProxy(index))
                                .padding(4),
                            )
                            .push(space::horizontal().width(Length::Fixed(8_f32))),
                    )
                    .style(styles::container::download_style(index))
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
                                    .style(styles::button::primary)
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
                text_input("Proxy url", &self.proxy_input)
                    .on_input(Message::ProxyUrlChanged)
                    .style(styles::text_input::url_input_style(UrlStatus::None))
                    .padding(6),
            );
        }

        Row::new()
            .push(space::horizontal().width(Length::Fixed(8_f32)))
            .push(column)
            .into()
    }
}
