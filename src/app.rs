use crate::config::Config;
use crate::helpers::*;
use crate::mega_client::MegaClient;
use crate::resources::*;
use crate::screens::*;
use crate::{Download, MegaFile, RunnerMessage, spawn_workers, styles};
use futures::future::join_all;
use futures::FutureExt;
use iced::alignment::{Horizontal, Vertical};
use iced::font::{Family, Weight};
use iced::time::every;
use iced::widget::{Column, Row, space, svg};
use iced::widget::{
    button, center, container, mouse_area, opaque, stack, text,
};
use iced::{Alignment, Color, Element, Font, Length, Subscription, Task, Theme};
use log::error;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender as TokioSender;
use tokio_util::sync::CancellationToken;

pub(crate) const MONOSPACE: Font = Font {
    family: Family::Name("Inconsolata"),
    weight: Weight::Medium,
    ..Font::DEFAULT
};

pub(crate) struct App {
    settings: Settings,
    import: Import,
    choose_files: Option<ChooseFiles>,
    home: Home,
    mega: MegaClient,
    worker: Option<WorkerState>,
    runner_sender: Option<TokioSender<RunnerMessage>>,
    download_sender: kanal::Sender<Download>,
    download_receiver: kanal::AsyncReceiver<Download>,
    file_handles: HashSet<String>,
    route: Route,
    error_modal: Option<String>,
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

        if !self.home.has_active_downloads() {
        } else {
            title.push_str(&format!(" - {} running", self.home.active_downloads().len()));
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
            Message::Home(msg) => {
                use crate::screens::home::Action;
                match self.home.update(msg) {
                    Action::None => Task::none(),
                    Action::StopWorkers => {
                        // Cancel queued downloads so they don't auto-start if workers restart.
                        self.drain_download_queue();
                        self.stop_workers();
                        Task::none()
                    }
                }
            }
            Message::Import(msg) => {
                use crate::screens::import::Action;
                match self.import.update(msg, &self.mega) {
                    Action::None => Task::none(),
                    Action::Run(task) => task.map(Message::Import),
                    Action::FilesLoaded(files) => {
                        // Filter duplicates using file_handles
                        let mut accepted: Vec<MegaFile> = Vec::new();
                        for file in files {
                            let handles: Vec<String> =
                                file.iter().map(|f| f.node.handle.clone()).collect();
                            let has_duplicate =
                                handles.iter().any(|h| self.file_handles.contains(h));
                            if !has_duplicate {
                                for handle in &handles {
                                    self.file_handles.insert(handle.clone());
                                }
                                accepted.push(file);
                            }
                        }

                        if !accepted.is_empty() {
                            if let Some(choose_files) = &mut self.choose_files {
                                choose_files.add_files(accepted);
                            } else {
                                self.choose_files = Some(ChooseFiles::new(accepted));
                            }
                        }
                        Task::none()
                    }
                    Action::ShowError(error) => {
                        self.error_modal = Some(error);
                        Task::none()
                    }
                }
            }
            Message::ChooseFiles(msg) => {
                use crate::screens::choose_files::Action;

                if let Some(choose_files) = &mut self.choose_files {
                    let active_handles: HashSet<String> =
                        self.home.active_downloads().keys().cloned().collect();
                    match choose_files.update(msg, &active_handles) {
                        Action::None => Task::none(),
                        Action::QueueDownloads(downloads) => {
                            // Queue downloads
                            for download in downloads {
                                self.download_sender.send(download).unwrap();
                            }
                            // Start workers if needed
                            if self.worker.is_none() {
                                self.worker =
                                    Some(self.start_workers(self.settings.config.max_workers));
                            }
                            // Navigate to home
                            self.route = Route::Home;
                            // Clear the screen
                            self.choose_files = None;
                            Task::perform(async {}, |_| Message::ClearFiles)
                        }
                        Action::ClearFiles => {
                            self.choose_files = None;
                            Task::perform(async {}, |_| Message::ClearFiles)
                        }
                    }
                } else {
                    Task::none()
                }
            }
            Message::RunnerReady(sender) => {
                self.runner_sender = Some(sender);
                Task::none()
            }
            Message::Runner(message) => {
                match message {
                    RunnerMessage::Active(download) => {
                        // add download to active downloads
                        self.home.add_active_download(download);
                    }
                    RunnerMessage::Inactive(id) => {
                        self.home.remove_active_download(&id);

                        // if there are no active downloads, stop the runner
                        if !self.home.has_active_downloads() && self.download_receiver.is_empty() {
                            self.stop_workers();
                        }
                    }
                    RunnerMessage::Error(error) => {
                        self.home.add_error(error);
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
                        if self.choose_files.is_none() {
                            self.error_modal = Some("No files imported".to_string())
                        } else {
                            self.route = route
                        }
                    }
                }

                Task::none()
            }
            Message::CloseModal => {
                self.error_modal = None;
                Task::none()
            }
            Message::Settings(msg) => {
                use crate::screens::settings::Action;
                match self.settings.update(msg) {
                    Action::None => Task::none(),
                    Action::ConfigSaved => Task::none(),
                    Action::RebuildRequired(config) => {
                        // if the worker is active, do not rebuild
                        if self.worker.is_some() {
                            self.error_modal = Some(
                                "Cannot apply these configuration changes while downloads are active"
                                    .to_string(),
                            );
                            return Task::none();
                        }

                        // build a new mega client
                        match mega_builder(&config) {
                            Ok(mega) => {
                                self.mega = mega; // set the new mega client
                                self.settings = Settings::new(config.clone());
                                self.settings.set_rebuild_available(false);
                                Task::perform(async {}, |_| {
                                    Message::Settings(SettingsMessage::SaveConfig)
                                }) // save the config
                            }
                            Err(error) => {
                                self.error_modal =
                                    Some(format!("Failed to build mega client: {}", error));
                                Task::none()
                            }
                        }
                    }
                    Action::ShowError(error) => {
                        self.error_modal = Some(error);
                        Task::none()
                    }
                }
            }
            Message::ClearFiles => {
                self.file_handles.clear(); // clear file handles tracking
                self.choose_files = None;

                // clear loaded URL inputs
                self.import.clear_loaded_inputs();

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
                container(
                    self.home
                        .view(&self.settings.config.get_theme())
                        .map(Message::Home),
                )
            }
            Route::Import => container(self.import.view().map(Message::Import)),
            Route::ChooseFiles => {
                if let Some(choose_files) = &self.choose_files {
                    container(
                        choose_files
                            .view(&self.settings.config.get_theme())
                            .map(Message::ChooseFiles),
                    )
                } else {
                    container(text("No files loaded"))
                }
            }
            Route::Settings => container(self.settings.view().map(Message::Settings)),
        };

        // nav + content = body
        let nav_theme = self.settings.config.get_theme();
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
                                self.choose_files.is_none(),
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
            let theme = self.settings.config.get_theme();
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
        Some(self.settings.config.get_theme())
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

    fn nav_button<'a>(
        &self,
        theme: &Theme,
        label: &'a str,
        route: Route,
        disabled: bool,
    ) -> Element<'a, Message> {
        let palette = theme.extended_palette();
        let color = if disabled {
            Some(palette.secondary.weak.color)
        } else {
            Some(palette.primary.strong.color)
        };

        let mut row = Row::new()
            .align_y(Alignment::Center)
            .height(Length::Fixed(40_f32));

        if self.route == route {
            let style = styles::svg::svg_icon_style(color);
            row = row
                .push(
                    svg(svg::Handle::from_memory(SELECTED_ICON))
                        .style(style)
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

        let svg_style = styles::svg::svg_icon_style(color);
        row = row
            .push(
                container(
                    svg(handle)
                        .width(Length::Fixed(28_f32))
                        .height(Length::Fixed(28_f32))
                        .style(svg_style),
                )
                .padding(4)
                .style({
                    let is_active = self.route == route;
                    styles::container::icon_style(is_active)
                }),
            )
            .push(space::horizontal().width(Length::Fixed(12_f32)));

        let mut button = button(row.push(text(label)))
            .style(styles::button::nav_style(self.route == route))
            .width(Length::Fill)
            .padding(0);

        if !disabled {
            button = button.on_press(Message::Navigate(route));
        }

        button.into()
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
                Arc::new(self.settings.config.clone()),
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

    fn drain_download_queue(&mut self) {
        loop {
            match self.download_receiver.recv().now_or_never() {
                Some(Ok(download)) => {
                    download.cancel();
                }
                // channel closed
                Some(Err(_)) => break,
                // nothing currently queued
                None => break,
            }
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
            settings: Settings::new(config),
            import: Import::new(),
            choose_files: None,
            home: Home::new(),
            mega,
            worker: None,
            runner_sender: None,
            download_sender,
            download_receiver: download_receiver.to_async(),
            file_handles: HashSet::new(),
            route: Route::Home,
            error_modal: None,
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
