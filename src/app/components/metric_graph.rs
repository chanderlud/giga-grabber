use crate::app::MONOSPACE;
use crate::app::styles;
use iced::widget::{Column, Row, canvas, container, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

const GRAPH_HEIGHT: f32 = 72.0;
const PADDING: f32 = 4.0;

pub(crate) struct Graph {
    pub(crate) label: &'static str,
    pub(crate) number: String,
    pub(crate) unit: Option<&'static str>,
    pub(crate) samples: Vec<f32>,
}

pub(crate) fn graph_cards<'a, Message: 'a>(
    graphs: impl IntoIterator<Item = Graph>,
) -> Element<'a, Message> {
    graphs
        .into_iter()
        .fold(Row::new().spacing(5).width(Length::Fill), |row, graph| {
            row.push(
                container(metric_graph(graph))
                    .style(styles::container::download_list_style())
                    .padding(8)
                    .width(Length::FillPortion(1)),
            )
        })
        .into()
}

fn metric_graph<'a, Message: 'a>(graph: Graph) -> Element<'a, Message> {
    let mut heading = Row::new()
        .spacing(8)
        .push(text(graph.label).size(14))
        .push(text(graph.number).font(MONOSPACE).size(14));
    if let Some(unit) = graph.unit {
        heading = heading.push(text(unit).size(14));
    }

    Column::new()
        .spacing(2)
        .push(heading)
        .push(
            canvas(MetricGraph {
                samples: graph.samples,
            })
            .width(Length::Fill)
            .height(Length::Fixed(GRAPH_HEIGHT)),
        )
        .into()
}

struct MetricGraph {
    samples: Vec<f32>,
}

impl<Message> canvas::Program<Message> for MetricGraph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let baseline = canvas::Path::line(
            Point::new(PADDING, bounds.height - PADDING),
            Point::new(bounds.width - PADDING, bounds.height - PADDING),
        );
        frame.stroke(
            &baseline,
            canvas::Stroke::default()
                .with_color(Color {
                    a: 0.35,
                    ..palette.background.strong.color
                })
                .with_width(1.0),
        );

        if self.samples.len() > 1 {
            let maximum = self
                .samples
                .iter()
                .copied()
                .fold(0.0_f32, f32::max)
                .max(1.0);
            let width = (bounds.width - (PADDING * 2.0)).max(1.0);
            let height = (bounds.height - (PADDING * 2.0)).max(1.0);
            let step = width / (self.samples.len() - 1) as f32;
            let line = canvas::Path::new(|builder| {
                for (index, sample) in self.samples.iter().enumerate() {
                    let point = Point::new(
                        PADDING + step * index as f32,
                        bounds.height - PADDING - (sample / maximum) * height,
                    );
                    if index == 0 {
                        builder.move_to(point);
                    } else {
                        builder.line_to(point);
                    }
                }
            });
            frame.stroke(
                &line,
                canvas::Stroke::default()
                    .with_color(palette.primary.strong.color)
                    .with_width(1.5),
            );
        }

        vec![frame.into_geometry()]
    }
}
