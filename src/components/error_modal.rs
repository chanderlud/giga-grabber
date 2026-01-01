use iced::alignment::{Horizontal, Vertical};
use iced::widget::{
    Column, Row, button, center, container, mouse_area, opaque, space, stack, text,
};
use iced::{Color, Element, Length, Theme};

pub(crate) fn error_modal<'a, Message: 'a>(
    error_message: &'a str,
    theme: &Theme,
    content: Element<'a, Message>,
) -> Element<'a, ()> {
    let error_color = theme.extended_palette().danger.strong.color;

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
                                    .color(error_color)
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
            .on_press(())
        )
    ]
    .into()
}
