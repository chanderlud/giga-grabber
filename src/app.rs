use crate::app::components::{modal, nav_sidebar};
use crate::app::helpers::*;
use crate::app::screens::choose_files::QueuedDownload;
use crate::app::screens::import::ImportedFiles;
use crate::app::screens::*;
use crate::config::Config;
use crate::mega_client::MegaClient;
use crate::session_persistence::{self, DownloadSessionRecord};
use crate::update_check::{self, ReleaseInfo, UpdateCheckError, UpdateStatus};
use crate::worker::mega_client::mega_builder;
use crate::{Download, MegaFile, NodeKind, RunnerMessage, SessionEvent, TransferSession};
use iced::font::{Family, Weight};
use iced::time::every;
use iced::widget::{Row, container, text};
use iced::{Element, Font, Length, Subscription, Task, Theme, window};
use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::Duration;
use tokio::sync::mpsc::Sender as TokioSender;

mod components;
mod helpers;
mod screens;
mod styles;

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
    download_records: HashMap<String, DownloadSessionRecord>,
    persist_download_sessions: bool,
    restore_started: bool,
    close_pending: bool,
    route: Route,
    error_modal: Option<String>,
    update_release: Option<ReleaseInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct RestoredDownloads {
    downloads: Vec<QueuedDownload>,
    retained_records: Vec<DownloadSessionRecord>,
    errors: Vec<String>,
}

impl App {
    fn new() -> (Self, Task<Message>) {
        // load config from disk, falling back to a default config if needed
        let (mut config, mut error_modal) = Config::new();
        let persist_download_sessions = config.persist_download_sessions;
        if !config.persist_download_sessions
            && let Err(error) = session_persistence::remove()
        {
            error_modal.get_or_insert_with(|| {
                format!("Failed to remove disabled download session: {error}")
            });
        }
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

        let check_for_updates = config.check_for_updates;
        let app = Self {
            settings: Settings::new(config),
            import: Import::new(),
            choose_files: None,
            home: Home::new(),
            mega,
            session: None,
            runner_sender: None,
            file_handles: HashSet::new(),
            download_records: HashMap::new(),
            persist_download_sessions,
            restore_started: false,
            close_pending: false,
            route: Route::Home,
            error_modal,
            update_release: None,
        };

        let task = if check_for_updates {
            Self::check_for_updates(false)
        } else {
            Task::none()
        };

        (app, task)
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
            Message::Home(msg) => match self.home.update(msg) {
                HomeAction::None => Task::none(),
                HomeAction::StopWorkers => {
                    self.remove_persisted_session();
                    self.download_records.clear();
                    if let Some(session) = &mut self.session {
                        session.abort_background();
                    }
                    self.session = None;
                    Task::none()
                }
            },
            Message::Import(msg) => match self.import.update(msg, &self.mega) {
                ImportAction::None => Task::none(),
                ImportAction::Run(task) => task.map(Message::Import),
                ImportAction::FilesLoaded(files) => {
                    let mut tracked_handles = self.file_handles.clone();
                    tracked_handles.extend(
                        self.session
                            .as_ref()
                            .map_or_else(HashSet::new, TransferSession::handles),
                    );
                    let mut accepted: Vec<MegaFile> = Vec::new();
                    for file in files.files {
                        let Some(file) = file.without_handles(&tracked_handles) else {
                            continue;
                        };

                        for handle in file.iter().map(|entry| entry.node.handle.clone()) {
                            tracked_handles.insert(handle.clone());
                            self.file_handles.insert(handle);
                        }

                        accepted.push(file);
                    }

                    if !accepted.is_empty() {
                        let accepted = ImportedFiles {
                            files: accepted,
                            source_url: files.source_url,
                        };
                        if let Some(choose_files) = &mut self.choose_files {
                            choose_files.add_files(vec![accepted]);
                        } else {
                            self.choose_files = Some(ChooseFiles::new(vec![accepted]));
                        }
                    }
                    Task::none()
                }
                ImportAction::ShowError(error) => {
                    self.error_modal = Some(error);
                    Task::none()
                }
            },
            Message::ChooseFiles(msg) => {
                if let Some(choose_files) = &mut self.choose_files {
                    let session_handles = self
                        .session
                        .as_ref()
                        .map_or_else(HashSet::new, TransferSession::handles);
                    match choose_files.update(msg, &session_handles) {
                        ChooseFilesAction::None => Task::none(),
                        ChooseFilesAction::QueueDownloads(downloads) => {
                            let Some(runner_sender) = self.runner_sender.clone() else {
                                self.error_modal =
                                    Some("Download runner is not ready yet".to_string());
                                return Task::none();
                            };

                            if downloads.is_empty() {
                                self.route = Route::Home;
                                self.choose_files = None;
                                return Task::perform(async {}, |_| Message::ClearFiles);
                            }

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
                                let existing_handles = session.handles();
                                let downloads_to_queue: Vec<Download> = downloads
                                    .iter()
                                    .map(|queued| queued.download.clone())
                                    .collect();
                                if let Err(error) = session.add_downloads(downloads_to_queue) {
                                    self.error_modal =
                                        Some(format!("Failed to queue downloads: {error}"));
                                    return Task::none();
                                }
                                let accepted_handles = session.handles();
                                self.record_accepted_downloads(
                                    &downloads,
                                    &existing_handles,
                                    &accepted_handles,
                                );
                            }

                            // Navigate to home
                            self.route = Route::Home;
                            // Clear the screen
                            self.choose_files = None;
                            Task::perform(async {}, |_| Message::ClearFiles)
                        }
                        ChooseFilesAction::ClearFiles => {
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
                if self.persist_download_sessions && !self.restore_started {
                    self.restore_started = true;
                    let mega = self.mega.clone();
                    Task::perform(restore_downloads(mega), Message::RestoreFinished)
                } else {
                    Task::none()
                }
            }
            Message::RestoreFinished(result) => match result {
                Ok(restored) if restored.downloads.is_empty() => {
                    self.retain_download_records(restored.retained_records);
                    if restored.errors.is_empty() {
                        self.remove_persisted_session();
                    } else {
                        self.home.add_error(restored.errors.join("\n"));
                    }
                    Task::none()
                }
                Ok(restored) => {
                    let Some(runner_sender) = self.runner_sender.clone() else {
                        self.error_modal = Some("Download runner is not ready yet".to_string());
                        return Task::none();
                    };

                    self.retain_download_records(restored.retained_records);
                    if self.session.is_none() {
                        self.session = Some(TransferSession::new(
                            self.mega.clone(),
                            self.settings.config.clone(),
                        ));
                    }

                    let session = self.session.as_mut().expect("session created when absent");
                    session.set_runner_sender(runner_sender);
                    let existing_handles = session.handles();
                    let downloads_to_queue: Vec<Download> = restored
                        .downloads
                        .iter()
                        .map(|queued| queued.download.clone())
                        .collect();
                    match session.add_downloads(downloads_to_queue) {
                        Ok(_) => {
                            let accepted_handles = session.handles();
                            self.record_accepted_downloads(
                                &restored.downloads,
                                &existing_handles,
                                &accepted_handles,
                            );
                            expose_accepted_downloads(
                                &mut self.home,
                                &restored.downloads,
                                &existing_handles,
                                &accepted_handles,
                            );
                            if !restored.errors.is_empty() {
                                self.home.add_error(restored.errors.join("\n"));
                            }
                        }
                        Err(error) => {
                            self.error_modal =
                                Some(format!("Failed to restore downloads: {error}"));
                        }
                    }
                    Task::none()
                }
                Err(error) => {
                    self.error_modal = Some(format!("Failed to restore downloads: {error}"));
                    Task::none()
                }
            },
            Message::CloseRequested => {
                if self.close_pending {
                    return Task::none();
                }
                self.close_pending = true;
                if !self.persist_download_sessions {
                    return self.close_window();
                }

                let records: Vec<_> = self.download_records.values().cloned().collect();
                Task::perform(
                    async move {
                        if records.is_empty() {
                            session_persistence::remove()
                        } else {
                            session_persistence::save(&records)
                        }
                        .map_err(|error| error.to_string())
                    },
                    Message::CloseSnapshotFinished,
                )
            }
            Message::CloseSnapshotFinished(result) => match result {
                Ok(()) => self.close_window(),
                Err(error) => {
                    self.close_pending = false;
                    self.error_modal = Some(format!("Failed to save download session: {error}"));
                    Task::none()
                }
            },
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
                self.update_release = None;
                Task::none()
            }
            Message::OpenUrl(url) => {
                self.update_release = None;
                if let Err(error) = open_url_in_browser(&url) {
                    self.error_modal = Some(error);
                }
                Task::none()
            }
            Message::Settings(msg) => {
                match self.settings.update(msg) {
                    SettingsAction::None => Task::none(),
                    SettingsAction::ConfigSaved => {
                        self.persist_download_sessions =
                            self.settings.config.persist_download_sessions;
                        if !self.persist_download_sessions {
                            self.remove_persisted_session();
                        }
                        Task::none()
                    }
                    SettingsAction::CheckForUpdates => Self::check_for_updates(true),
                    SettingsAction::RebuildRequired(config) => {
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
                    SettingsAction::ShowError(error) => {
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
            Message::UpdateCheckFinished { manual, result } => {
                match result {
                    Ok(UpdateStatus::Available(release)) => {
                        self.update_release = Some(release);
                    }
                    Ok(UpdateStatus::Current) if manual => {
                        self.error_modal = Some("Giga Grabber is up to date".to_string());
                    }
                    Err(error) if manual => {
                        self.error_modal = Some(format!("Failed to check for updates: {error}"));
                    }
                    Ok(UpdateStatus::Current) | Err(_) => {}
                }

                Task::none()
            }
        }
    }

    fn check_for_updates(manual: bool) -> Task<Message> {
        Task::perform(
            async {
                update_check::check_latest_release()
                    .await
                    .map_err(UpdateCheckError::new)
            },
            move |result| Message::UpdateCheckFinished { manual, result },
        )
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

        if let Some(release) = &self.update_release {
            modal::update_modal(
                &release.version,
                body.into(),
                Message::OpenUrl(release.url.clone()),
                Message::CloseModal,
            )
        } else if let Some(error_message) = &self.error_modal {
            modal::error_modal(error_message, body.into()).map(|_| Message::CloseModal)
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
        let close_requests = window::close_requests().map(|_| Message::CloseRequested);

        // run all subscriptions in parallel
        Subscription::batch(vec![runner_subscription, refresh, close_requests])
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
                        self.download_records.remove(&id);
                    }
                    SessionEvent::Error(error) => {
                        self.home.add_error(error);
                    }
                    SessionEvent::OutOfBandwidth(error) => {
                        self.home.add_error(error.clone());
                        self.home.update(HomeMessage::PauseDownloads);
                        self.error_modal = Some(error);
                    }
                    SessionEvent::Drained => {
                        drained = true;
                    }
                }
            }
        }

        if drained {
            self.download_records.clear();
            self.remove_persisted_session();
            if let Some(session) = &mut self.session {
                session.finish_background();
            }
            self.session = None;
        }
    }

    fn record_accepted_downloads(
        &mut self,
        downloads: &[QueuedDownload],
        existing_handles: &HashSet<String>,
        accepted_handles: &HashSet<String>,
    ) {
        for (handle, record) in records_by_handle(downloads) {
            if !existing_handles.contains(&handle) && accepted_handles.contains(&handle) {
                self.download_records.insert(handle, record);
            }
        }
    }

    fn retain_download_records(&mut self, records: Vec<DownloadSessionRecord>) {
        retain_download_records(&mut self.download_records, records);
    }

    fn remove_persisted_session(&mut self) {
        if let Err(error) = session_persistence::remove() {
            self.error_modal = Some(format!("Failed to remove download session: {error}"));
        }
    }

    fn close_window(&mut self) -> Task<Message> {
        if let Some(session) = &mut self.session {
            session.abort_background();
        }
        self.session = None;
        window::latest().and_then(window::close)
    }
}

fn records_by_handle(downloads: &[QueuedDownload]) -> HashMap<String, DownloadSessionRecord> {
    downloads
        .iter()
        .map(|queued| {
            (
                queued.download.node.handle.clone(),
                DownloadSessionRecord {
                    source_url: queued.source_url.clone(),
                    node_handle: queued.download.node.handle.clone(),
                    destination_dir: queued.download.file_path.clone(),
                    expected_size: queued.download.node.size,
                },
            )
        })
        .collect()
}

fn retain_download_records(
    download_records: &mut HashMap<String, DownloadSessionRecord>,
    records: Vec<DownloadSessionRecord>,
) {
    for record in records {
        download_records
            .entry(record.node_handle.clone())
            .or_insert(record);
    }
}

fn expose_accepted_downloads(
    home: &mut Home,
    downloads: &[QueuedDownload],
    existing_handles: &HashSet<String>,
    accepted_handles: &HashSet<String>,
) {
    for queued in downloads {
        let handle = &queued.download.node.handle;
        if !existing_handles.contains(handle) && accepted_handles.contains(handle) {
            home.add_active_download(queued.download.clone());
        }
    }
}

async fn restore_downloads(mega: MegaClient) -> Result<RestoredDownloads, String> {
    let records = session_persistence::load().map_err(|error| error.to_string())?;
    let mut records_by_url: HashMap<String, Vec<DownloadSessionRecord>> = HashMap::new();
    for record in records {
        records_by_url
            .entry(record.source_url.clone())
            .or_default()
            .push(record);
    }

    let mut downloads = Vec::new();
    let mut retained_records = Vec::new();
    let mut errors = Vec::new();
    for (source_url, records) in records_by_url {
        let nodes = match mega.fetch_public_nodes(&source_url).await {
            Ok(nodes) => nodes,
            Err(error) => {
                errors.push(format!("Failed to restore {source_url}: {error}"));
                retained_records.extend(records);
                continue;
            }
        };
        for record in records {
            let Some(node) = nodes.get(&record.node_handle) else {
                continue;
            };
            if node.kind != NodeKind::File || node.size != record.expected_size {
                continue;
            }
            let file = MegaFile::new(node.clone(), record.destination_dir);
            let download = Download::restore(&file)
                .await
                .map_err(|error| format!("Failed to inspect {}: {error}", node.name))?;
            if let Some(download) = download {
                downloads.push(QueuedDownload {
                    download,
                    source_url: record.source_url,
                });
            }
        }
    }
    Ok(RestoredDownloads {
        downloads,
        retained_records,
        errors,
    })
}

fn open_url_in_browser(url: &str) -> Result<(), String> {
    let mut command = browser_command(url);
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "Failed to open release page: browser exited with {status}"
        )),
        Err(error) => Err(format!("Failed to open release page: {error}")),
    }
}

#[cfg(target_os = "macos")]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}

#[cfg(target_os = "windows")]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("cmd");
    command.args(["/C", "start", "", url]);
    command
}

#[cfg(all(unix, not(target_os = "macos")))]
fn browser_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
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

#[cfg(test)]
mod tests {
    use super::{expose_accepted_downloads, records_by_handle, retain_download_records};
    use crate::app::screens::choose_files::QueuedDownload;
    use crate::app::screens::home::Home;
    use crate::mega_client::Node;
    use crate::session_persistence::DownloadSessionRecord;
    use crate::{Download, MegaFile};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    #[test]
    fn accepted_queue_records_keep_restore_provenance() {
        let file = MegaFile::new(
            Node::test_file("A1b2C3", "example-video.mkv", 2_097_152),
            PathBuf::from("courses/rust"),
        );
        let queued = QueuedDownload {
            download: Download::new(&file),
            source_url: "https://mega.nz/folder/AbCdEf#trusted-key".to_string(),
        };

        let records = records_by_handle(&[queued]);
        let record = records
            .get("A1b2C3")
            .expect("record for accepted queue item");

        assert_eq!(
            record.source_url,
            "https://mega.nz/folder/AbCdEf#trusted-key"
        );
        assert_eq!(record.node_handle, "A1b2C3");
        assert_eq!(record.destination_dir, PathBuf::from("courses/rust"));
        assert_eq!(record.expected_size, 2_097_152);
    }

    #[test]
    fn failed_restore_records_survive_without_replacing_queued_records() {
        let mut records = HashMap::from([(
            "active-handle".to_string(),
            DownloadSessionRecord {
                source_url: "https://mega.nz/file/active#key".to_string(),
                node_handle: "active-handle".to_string(),
                destination_dir: PathBuf::from("downloads/active"),
                expected_size: 4_096,
            },
        )]);

        retain_download_records(
            &mut records,
            vec![
                DownloadSessionRecord {
                    source_url: "https://mega.nz/file/stale#key".to_string(),
                    node_handle: "active-handle".to_string(),
                    destination_dir: PathBuf::from("downloads/stale"),
                    expected_size: 4_096,
                },
                DownloadSessionRecord {
                    source_url: "https://mega.nz/folder/retry#key".to_string(),
                    node_handle: "retry-handle".to_string(),
                    destination_dir: PathBuf::from("downloads/retry"),
                    expected_size: 8_192,
                },
            ],
        );

        assert_eq!(records.len(), 2);
        assert_eq!(
            records["active-handle"].source_url,
            "https://mega.nz/file/active#key"
        );
        assert_eq!(
            records["retry-handle"].source_url,
            "https://mega.nz/folder/retry#key"
        );
    }

    #[test]
    fn accepted_restored_download_is_visible_with_seeded_progress() {
        let file = MegaFile::new(
            Node::test_file("restored-handle", "restored.bin", 1_024),
            PathBuf::from("saved"),
        );
        let download = Download::new(&file);
        download.set_downloaded(256);
        download.pause();
        let queued = QueuedDownload {
            download,
            source_url: "https://mega.nz/file/restored#key".to_string(),
        };
        let mut home = Home::new();

        expose_accepted_downloads(
            &mut home,
            &[queued],
            &HashSet::new(),
            &HashSet::from(["restored-handle".to_string()]),
        );

        let visible = &home.active_downloads()["restored-handle"];
        assert!(visible.is_paused());
        assert_eq!(visible.downloaded.load(Ordering::Relaxed), 256);
        assert_eq!(visible.progress(), 0.25);
    }
}
