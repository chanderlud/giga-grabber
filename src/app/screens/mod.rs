pub(crate) mod choose_files;
pub(crate) mod home;
pub(crate) mod import;
pub(crate) mod settings;

pub(crate) use choose_files::{
    Action as ChooseFilesAction, ChooseFiles, Message as ChooseFilesMessage,
};
pub(crate) use home::{Action as HomeAction, Home, Message as HomeMessage};
pub(crate) use import::{Action as ImportAction, Import, Message as ImportMessage};
pub(crate) use settings::{Action as SettingsAction, Message as SettingsMessage, Settings};

pub(crate) const CHECK_ICON: &[u8] = include_bytes!("../../../assets/check.svg");
pub(crate) const COLLAPSE_ICON: &[u8] = include_bytes!("../../../assets/collapse.svg");
pub(crate) const EXPAND_ICON: &[u8] = include_bytes!("../../../assets/expand.svg");
pub(crate) const SELECTED_ICON: &[u8] = include_bytes!("../../../assets/selector.svg");
pub(crate) const IMPORT_ICON: &[u8] = include_bytes!("../../../assets/import.svg");
pub(crate) const CHOOSE_ICON: &[u8] = include_bytes!("../../../assets/choose.svg");
pub(crate) const SETTINGS_ICON: &[u8] = include_bytes!("../../../assets/settings.svg");
pub(crate) const HOME_ICON: &[u8] = include_bytes!("../../../assets/home.svg");
pub(crate) const TRASH_ICON: &[u8] = include_bytes!("../../../assets/trash.svg");
pub(crate) const X_ICON: &[u8] = include_bytes!("../../../assets/x.svg");
pub(crate) const PAUSE_ICON: &[u8] = include_bytes!("../../../assets/pause.svg");
pub(crate) const PLAY_ICON: &[u8] = include_bytes!("../../../assets/play.svg");
pub(crate) const INCONSOLATA_MEDIUM: &[u8] =
    include_bytes!("../../../assets/Inconsolata/static/Inconsolata-Medium.ttf");
pub(crate) const CABIN_REGULAR: &[u8] =
    include_bytes!("../../../assets/Cabin/static/Cabin-Regular.ttf");
