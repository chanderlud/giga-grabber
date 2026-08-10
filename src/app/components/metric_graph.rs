use crate::app::MONOSPACE;
use iced::widget::{Column, canvas, text};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};

const GRAPH_HEIGHT: f32 = 72.0;
const PADDING: f32 = 4.0;

pub(crate) fn metric_graph<'a, Message: 'a>(
    label: &'a str,
    value: String,
    samples: Vec<f32>,
) -> Element<'a, Message> {
    Column::new()
        .spacing(2)
        .push(text(format!("{label}  {value}")).font(MONOSPACE).size(14))
        .push(
            canvas(MetricGraph { samples })
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
