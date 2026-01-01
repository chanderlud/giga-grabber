use crate::ProxyMode;
use crate::app::MONOSPACE;
use crate::config::Config;
use crate::helpers::{UrlStatus, pad_usize};
use crate::resources::X_ICON;
use crate::styles;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    Column, Row, button, container, pick_list, scrollable, slider, space, svg, text, text_input,
};
use iced::{Element, Length, Theme};
use native_dialog::FileDialog;
use num_traits::cast::ToPrimitive;
use regex::Regex;
use std::borrow::Cow;
use std::io::Read;
use std::ops::RangeInclusive;
use std::time::Duration;

#[derive(Clone)]
pub struct Settings {
    pub config: Config,
    pub rebuild_available: bool,
    pub proxy_regex: Regex,
}

#[derive(Debug, Clone)]
pub enum Message {
    SettingsSlider((usize, f64)),
    SaveConfig,
    ResetConfig,
    ThemeChanged(Theme),
    ProxyModeChanged(ProxyMode),
    ProxyUrlChanged(String),
    AddProxies,
    RemoveProxy(usize),
    RebuildMega,
}

pub enum Action {
    None,
    ConfigSaved,
    RebuildRequired(Config),
    ShowError(String),
}

impl Settings {
    pub fn new(config: Config) -> Self {
        Self {
            proxy_regex: Regex::new(r"^(http|https|socks5)://").unwrap(),
            rebuild_available: false,
            config,
        }
    }

    pub fn set_rebuild_available(&mut self, flag: bool) {
        self.rebuild_available = flag;
    }

    pub fn update(&mut self, message: Message) -> Action {
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
                for proxy in &self.config.proxies {
                    if !self.proxy_regex.is_match(proxy) {
                        return Action::ShowError(format!("Invalid proxy url: {}", proxy));
                    }
                }

                if let Err(error) = self.config.save() {
                    return Action::ShowError(format!("Failed to save configuration: {}", error));
                }

                Action::ConfigSaved
            }
            Message::ResetConfig => {
                self.config = Config::default();
                self.rebuild_available = true;
                Action::None
            }
            Message::ThemeChanged(theme) => {
                self.config.set_theme(theme);
                Action::None
            }
            Message::ProxyModeChanged(proxy_mode) => {
                if proxy_mode == ProxyMode::Single {
                    self.config.proxies.truncate(1);
                } else if proxy_mode == ProxyMode::None {
                    self.config.proxies.clear();
                }
                self.config.proxy_mode = proxy_mode;
                self.rebuild_available = true;
                Action::None
            }
            Message::ProxyUrlChanged(value) => {
                if let Some(proxy_url) = self.config.proxies.get_mut(0) {
                    *proxy_url = value;
                } else {
                    self.config.proxies.push(value);
                }
                self.rebuild_available = true;
                Action::None
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

                            let mut any_added = false;
                            for proxy in contents.lines() {
                                if self.proxy_regex.is_match(proxy) {
                                    self.config.proxies.push(proxy.to_string());
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
                for proxy in &self.config.proxies {
                    if !self.proxy_regex.is_match(proxy) {
                        return Action::ShowError(format!("Invalid proxy url: {}", proxy));
                    }
                }

                Action::RebuildRequired(self.config.clone())
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
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
