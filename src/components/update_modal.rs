use crate::styles;
use crate::update_check::UpdateInfo;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    Column, Row, button, center, container, mouse_area, opaque, space, stack, text,
};
use iced::{Color, Element, Length, border};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notification {
    Available(UpdateInfo),
    Current {
        current_version: String,
        latest_version: String,
    },
    Failed(String),
    InProgress,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    Dismiss,
    OpenRelease,
}

impl Notification {
    pub(crate) fn release_url(&self) -> Option<&str> {
        match self {
            Self::Available(info) => Some(&info.release_url),
            Self::Current { .. } | Self::Failed(_) | Self::InProgress => None,
        }
    }
}

pub(crate) fn update_modal<'a, AppMessage: 'a>(
    notification: &'a Notification,
    content: Element<'a, AppMessage>,
) -> Element<'a, Message> {
    stack![
        content.map(|_| Message::Dismiss),
        opaque(
            mouse_area(center(opaque(modal_content(notification))).style(|_theme| {
                container::Style {
                    background: Some(
                        Color {
                            a: 0.5,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..Default::default()
                }
            }))
            .on_press(Message::Dismiss)
        )
    ]
    .into()
}

fn modal_content(notification: &Notification) -> Element<'_, Message> {
    let (title, body) = notification_copy(notification);
    let mut actions = Row::new()
        .spacing(5)
        .push(space::horizontal().width(Length::Fill));

    if matches!(notification, Notification::Available(_)) {
        actions = actions.push(
            button(" Release page ")
                .style(styles::button::primary)
                .on_press(Message::OpenRelease),
        );
    }

    actions = actions.push(
        button(" Ok ")
            .style(styles::button::warning)
            .on_press(Message::Dismiss),
    );

    container(
        Column::new()
            .spacing(8)
            .push(
                text(title)
                    .size(20)
                    .align_y(Vertical::Center)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .push(
                text(body)
                    .size(16)
                    .align_y(Vertical::Center)
                    .align_x(Horizontal::Center)
                    .width(Length::Fill),
            )
            .push(actions),
    )
    .width(Length::Fixed(360_f32))
    .padding(12)
    .style(|theme| {
        let palette = theme.extended_palette();

        container::Style {
            background: Some(palette.background.weak.color.into()),
            text_color: Some(palette.background.weak.text),
            border: border::rounded(8),
            ..Default::default()
        }
    })
    .into()
}

fn notification_copy(notification: &Notification) -> (&'static str, String) {
    match notification {
        Notification::Available(info) => (
            "Update available",
            format!(
                "Giga Grabber {} is available. You are running {}.",
                info.latest_version, info.current_version
            ),
        ),
        Notification::Current {
            current_version,
            latest_version,
        } => (
            "You're up to date",
            format!("Giga Grabber {current_version} is current. Latest release: {latest_version}."),
        ),
        Notification::Failed(message) => (
            "Update check failed",
            format!("Could not check for updates: {message}"),
        ),
        Notification::InProgress => (
            "Update check in progress",
            "An update check is already running.".to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_notification_exposes_release_url() {
        let notification = Notification::Available(UpdateInfo {
            current_version: "1.3.2".to_string(),
            latest_version: "1.3.3".to_string(),
            tag: "v1.3.3".to_string(),
            release_url: "https://github.com/chanderlud/giga-grabber/releases/tag/v1.3.3"
                .to_string(),
        });

        assert_eq!(
            notification.release_url(),
            Some("https://github.com/chanderlud/giga-grabber/releases/tag/v1.3.3")
        );
    }

    #[test]
    fn non_available_notifications_do_not_expose_release_url() {
        assert_eq!(Notification::InProgress.release_url(), None);
        assert_eq!(
            Notification::Current {
                current_version: "1.3.2".to_string(),
                latest_version: "1.3.2".to_string(),
            }
            .release_url(),
            None
        );
    }
}
