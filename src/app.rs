use crate::components::{error_modal, nav_sidebar};
use crate::config::Config;
use crate::helpers::*;
use crate::mega_client::MegaClient;
use crate::screens::*;
use crate::{MegaFile, RunnerMessage, SessionEvent, TransferSession};
use iced::font::{Family, Weight};
use iced::time::every;
use iced::widget::{Row, container, text};
use iced::{Element, Font, Length, Subscription, Task, Theme};
use std::collections::HashSet;
use std::time::Duration;
use tokio::sync::mpsc::Sender as TokioSender;

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
    session: Option<TransferSession<MegaClient>>,
    runner_sender: Option<TokioSender<RunnerMessage>>,
    file_handles: HashSet<String>,
    route: Route,
    error_modal: Option<String>,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        // load config from disk, falling back to a default config if needed
        let (mut config, mut error_modal) = Config::new();
        // build the mega client, falling back to default config if needed
        let mega = loop {
            if let Ok(client) = mega_builder(&config) {
                break client;
            } else {
                error_modal =
                    Some("Invalid config loaded from disk, applying default options".to_string());
                config = Config::default();
            }
        };

        (
            Self {
                settings: Settings::new(config),
                import: Import::new(),
                choose_files: None,
                home: Home::new(),
                mega,
                session: None,
                runner_sender: None,
                file_handles: HashSet::new(),
                route: Route::Home,
                error_modal,
            },
            Task::none(),
        )
    }

    fn title(&self) -> String {
        let mut title = String::from("Giga Grabber");

        // runner is None when not in use
        if self
            .session
            .as_ref()
            .is_some_and(TransferSession::is_running)
        {
            title.push_str(" - downloads active");
        }

        if !self.home.has_active_downloads() {
        } else {
            title.push_str(&format!(
                " - {} running",
                self.home.active_downloads().len()
            ));
        }

        let queued = self
            .session
            .as_ref()
            .map_or(0, TransferSession::pending_count);
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
                        if let Some(session) = &mut self.session {
                            session.abort_background();
                        }
                        self.session = None;
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
                        let session_handles = self
                            .session
                            .as_ref()
                            .map_or_else(HashSet::new, TransferSession::handles);
                        // Filter duplicates using file_handles
                        let mut accepted: Vec<MegaFile> = Vec::new();
                        for file in files {
                            let handles: Vec<String> =
                                file.iter().map(|f| f.node.handle.clone()).collect();
                            let has_duplicate = handles.iter().any(|h| {
                                self.file_handles.contains(h) || session_handles.contains(h)
                            });
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
                    let session_handles = self
                        .session
                        .as_ref()
                        .map_or_else(HashSet::new, TransferSession::handles);
                    match choose_files.update(msg, &session_handles) {
                        Action::None => Task::none(),
                        Action::QueueDownloads(downloads) => {
                            let Some(runner_sender) = self.runner_sender.clone() else {
                                self.error_modal =
                                    Some("Download runner is not ready yet".to_string());
                                return Task::none();
                            };

                            if self.session.is_none() {
                                let mut session = TransferSession::new(
                                    self.mega.clone(),
                                    self.settings.config.clone(),
                                );
                                session.set_runner_sender(runner_sender.clone());
                                self.session = Some(session);
                            }

                            if let Some(session) = &mut self.session {
                                session.set_runner_sender(runner_sender);
                                if let Err(error) = session.add_downloads(downloads) {
                                    self.error_modal =
                                        Some(format!("Failed to queue downloads: {error}"));
                                    return Task::none();
                                }
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
                if let Some(session) = &mut self.session {
                    session.set_runner_sender(sender.clone());
                }
                self.runner_sender = Some(sender);
                Task::none()
            }
            Message::RunnerBatch(messages) => {
                for message in messages {
                    self.handle_runner_message(message);
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
                        if self
                            .session
                            .as_ref()
                            .is_some_and(TransferSession::has_live_transfers)
                        {
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
            Route::Home => container(self.home.view().map(Message::Home)),
            Route::Import => container(self.import.view().map(Message::Import)),
            Route::ChooseFiles => {
                if let Some(choose_files) = &self.choose_files {
                    container(choose_files.view().map(Message::ChooseFiles))
                } else {
                    container(text("No files loaded"))
                }
            }
            Route::Settings => container(self.settings.view().map(Message::Settings)),
        };

        // nav + content = body
        let body = container(
            Row::new()
                .push(
                    nav_sidebar::nav_sidebar(&self.route, self.choose_files.is_none())
                        .map(Message::Navigate),
                )
                .push(content.padding(10).width(Length::Fill)),
        )
        .width(Length::Fill)
        .height(Length::Fill);

        if let Some(error_message) = &self.error_modal {
            error_modal::error_modal(error_message, body.into()).map(|_| Message::CloseModal)
        } else {
            body.into()
        }
    }

    fn theme(&self) -> Option<Theme> {
        // Return None for system theme
        self.settings.config.get_theme()
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

    fn handle_runner_message(&mut self, message: RunnerMessage) {
        let mut drained = false;

        if let Some(session) = &mut self.session {
            for event in session.handle_runner_message(message) {
                match event {
                    SessionEvent::TransferActive(download) => {
                        self.home.add_active_download(download);
                    }
                    SessionEvent::TransferTerminal(id) => {
                        self.home.remove_active_download(&id);
                    }
                    SessionEvent::Error(error) => {
                        self.home.add_error(error);
                    }
                    SessionEvent::Drained => {
                        drained = true;
                    }
                }
            }
        }

        if drained {
            if let Some(session) = &mut self.session {
                session.finish_background();
            }
            self.session = None;
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
