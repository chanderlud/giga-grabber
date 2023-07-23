use iced::{Color, Point, Rectangle, Theme, Vector};
use iced::widget::canvas::{Fill, Frame};
use iced::widget::canvas::{Cursor, Geometry, Path, Program};

use crate::app::Message;

struct Circle {
    radius: f32,
    position: Vector,
}

pub(crate) struct LoadingWheel {
    angle: f32,
    circles: Vec<Circle>,
}

impl LoadingWheel {
    pub(crate) fn new(angle: f32) -> Self {
        Self {
            angle,
            // default, non rotated circles
            circles: vec![
                Circle {
                    radius: 3.0,
                    position: Vector::new(10.0, 0.0),
                },
                Circle {
                    radius: 2.8,
                    position: Vector::new(7.07, -7.07),
                },
                Circle {
                    radius: 2.6,
                    position: Vector::new(0.0, -10.0),
                },
                Circle {
                    radius: 2.4,
                    position: Vector::new(-7.07, -7.07),
                },
                Circle {
                    radius: 2.0,
                    position: Vector::new(-10.0, 0.0),
                },
                Circle {
                    radius: 1.8,
                    position: Vector::new(-7.07, 7.07),
                },
                Circle {
                    radius: 1.4,
                    position: Vector::new(0.0, 10.0),
                },
                Circle {
                    radius: 1.0,
                    position: Vector::new(7.07, 7.07),
                },
            ],
        }
    }
}

impl Program<Message> for LoadingWheel {
    type State = ();

    fn draw(&self, _state: &(), _theme: &Theme, bounds: Rectangle, _cursor: Cursor) -> Vec<Geometry> {
        let mut frame = Frame::new(bounds.size()); // create a new frame with the size of the bounds
        let center = Point::new(bounds.width / 2.0, bounds.height / 2.0); // get the center of the bounds
        let (sin, cos) = self.angle.sin_cos(); // get the sin and cos of the angle

        // rotate the circles
        for circle in &self.circles {
            // create an offset vector for the circle
            let offset = Vector::new(circle.position.x * cos - circle.position.y * sin + center.x, circle.position.x * sin + circle.position.y * cos + center.y);
            // create a circle path at the offset location
            let path = Path::circle(Point { x: offset.x, y: offset.y }, circle.radius);
            // fill the path with a color
            frame.fill(&path, Fill::from(Color::from_rgb8(53, 0, 211)));
        }

        vec![frame.into_geometry()]
    }
}
