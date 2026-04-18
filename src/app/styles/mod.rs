pub(crate) mod button;
pub(crate) mod container;
pub(crate) mod pick_list;
pub(crate) mod progress_bar;
pub(crate) mod slider;
pub(crate) mod svg;
pub(crate) mod text_input;

/// Returns `true` when the active [`iced::Theme`] is the custom "Vanilla" theme.
pub(crate) fn is_vanilla(theme: &iced::Theme) -> bool {
    use iced::theme::Base;
    theme.name() == "Vanilla"
}
