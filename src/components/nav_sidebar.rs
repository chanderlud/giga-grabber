use crate::helpers::Route;
use crate::resources::{CHOOSE_ICON, HOME_ICON, IMPORT_ICON, SELECTED_ICON, SETTINGS_ICON};
use crate::styles;
use iced::widget::{Column, Row, button, container, space, svg, text};
use iced::{Alignment, Element, Length, Theme};

pub(crate) fn nav_sidebar(
    current_route: &Route,
    choose_files_disabled: bool,
) -> Element<'static, Route> {
    container(
        Column::new()
            .padding(4)
            .spacing(4)
            .push(nav_button("Home", Route::Home, current_route, false))
            .push(nav_button("Import", Route::Import, current_route, false))
            .push(nav_button(
                "Choose files",
                Route::ChooseFiles,
                current_route,
                choose_files_disabled,
            ))
            .push(space::vertical().height(Length::Fill))
            .push(nav_button(
                "Settings",
                Route::Settings,
                current_route,
                false,
            )),
    )
    .width(Length::Fixed(170_f32))
    .height(Length::Fill)
    .style(|theme: &Theme| {
        let palette = theme.extended_palette();
        container::Style {
            background: Some(palette.background.strong.color.into()),
            ..Default::default()
        }
    })
    .into()
}

pub(crate) fn nav_button<'a>(
    label: &'a str,
    route: Route,
    current_route: &Route,
    disabled: bool,
) -> Element<'a, Route> {
    let mut row = Row::new()
        .align_y(Alignment::Center)
        .height(Length::Fixed(40_f32));

    if current_route == &route {
        let style = styles::svg::nav_svg(disabled);
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

    row = row
        .push(
            container(
                svg(handle)
                    .width(Length::Fixed(28_f32))
                    .height(Length::Fixed(28_f32))
                    .style(styles::svg::nav_svg(disabled)),
            )
            .padding(4)
            .style({
                let is_active = current_route == &route;
                styles::container::icon_style(is_active)
            }),
        )
        .push(space::horizontal().width(Length::Fixed(12_f32)));

    let mut b = button(row.push(text(label)))
        .style(styles::button::navigation(current_route == &route))
        .width(Length::Fill)
        .padding(0);

    if !disabled {
        b = b.on_press(route);
    }

    b.into()
}
