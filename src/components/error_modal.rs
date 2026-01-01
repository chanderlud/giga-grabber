use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    Column, Row, button, center, container, mouse_area, opaque, space, stack, text,
};
use iced::{Color, Element, Length, border};

pub(crate) fn error_modal<'a, Message: 'a>(
    error_message: &'a str,
    content: Element<'a, Message>,
) -> Element<'a, ()> {
    stack![
        content.map(|_| ()),
        opaque(
            mouse_area(
                center(opaque(
                    container(
                        Column::new()
                            .spacing(5)
                            .push(
                                text(error_message)
                                    .size(18)
                                    .align_y(Vertical::Center)
                                    .align_x(Horizontal::Center),
                            )
                            .push(space::horizontal().width(Length::Fixed(100_f32)))
                            .push(
                                Row::new()
                                    .spacing(5)
                                    .push(space::horizontal().width(Length::FillPortion(3)))
                                    .push(button(" Ok ").style(button::primary).on_press(())),
                            ),
                    )
                    .width(Length::Fixed(250_f32))
                    .padding(10)
                    .style(|theme| {
                        let palette = theme.extended_palette();

                        container::Style {
                            background: Some(palette.background.weak.color.into()),
                            text_color: Some(palette.background.weak.text),
                            border: border::rounded(8),
                            ..Default::default()
                        }
                    })
                ))
                .style(|_theme| container::Style {
                    background: Some(
                        Color {
                            a: 0.5,
                            ..Color::BLACK
                        }
                        .into(),
                    ),
                    ..Default::default()
                })
            )
            .on_press(())
        )
    ]
    .into()
}
