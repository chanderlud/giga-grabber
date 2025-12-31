//! Show a loading wheel with rotating dots.
use iced::advanced::layout;
use iced::advanced::renderer;
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{self, Clipboard, Layout, Shell, Widget};
use iced::mouse;
use iced::time::Instant;
use iced::widget::canvas;
use iced::window;
use iced::{Color, Element, Event, Length, Rectangle, Renderer, Size, Vector};

use std::f32::consts::PI;

/// A circle representing a single dot in the loading wheel
struct Circle {
    radius: f32,
    position: Vector,
}

/// Canvas program that draws the loading wheel with rotating dots
struct LoadingWheelProgram {
    circles: Vec<Circle>,
}

impl LoadingWheelProgram {
    /// Creates a new [`LoadingWheelProgram`] with 8 dots positioned around a circle
    fn new() -> Self {
        let mut circles = Vec::new();
        let base_radius = 10.0;

        // Exact radii sequence matching the original dot-based style
        let radii = [3.0, 2.8, 2.6, 2.4, 2.0, 1.8, 1.4, 1.0];

        // Create 8 circles positioned at 45-degree intervals
        for i in 0..8 {
            let angle = (i as f32) * PI / 4.0;
            let radius = radii[i];
            let position = Vector::new(angle.cos() * base_radius, angle.sin() * base_radius);
            circles.push(Circle { radius, position });
        }

        Self { circles }
    }
}

/// Widget wrapper for the LoadingWheel canvas program
pub(crate) struct LoadingWheelWidget {
    program: LoadingWheelProgram,
    size: f32,
}

impl LoadingWheelWidget {
    /// Creates a new [`LoadingWheelWidget`] with default size of 40.0
    pub(crate) fn new() -> Self {
        Self {
            program: LoadingWheelProgram::new(),
            size: 40.0,
        }
    }

    /// Sets the size of the [`LoadingWheelWidget`]
    pub(crate) fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }
}

impl Default for LoadingWheelWidget {
    fn default() -> Self {
        Self::new()
    }
}

/// State for managing animation
#[derive(Default)]
struct State {
    angle: f32,
    cache: canvas::Cache,
    last_update: Option<Instant>,
}

impl<'a, Message> Widget<Message, iced::Theme, Renderer> for LoadingWheelWidget
where
    Message: 'a + Clone,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.size),
            height: Length::Fixed(self.size),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.size, self.size)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use advanced::Renderer as _;

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            let center = frame.center();

            // Apply rotation transformation
            let cos = state.angle.cos();
            let sin = state.angle.sin();

            // Draw each circle
            for circle in &self.program.circles {
                // Rotate the position vector
                let rotated_x = circle.position.x * cos - circle.position.y * sin;
                let rotated_y = circle.position.x * sin + circle.position.y * cos;

                let position = Vector::new(rotated_x, rotated_y);
                let circle_center = center + position;

                let path = canvas::Path::circle(circle_center, circle.radius);
                frame.fill(&path, Color::from_rgb8(255, 48, 78));
            }
        });

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            use iced::advanced::graphics::geometry::Renderer as _;

            renderer.draw_geometry(geometry);
        });
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();

        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            if let Some(last_update) = state.last_update {
                let elapsed = now.duration_since(last_update);
                // Increment angle proportionally (2π per second)
                state.angle += elapsed.as_secs_f32() * 2.0 * PI;
                // Wrap angle at 2π to prevent overflow
                state.angle = state.angle % (2.0 * PI);
            }
            state.last_update = Some(*now);
            state.cache.clear();
            shell.request_redraw();
        }
    }
}

impl<'a, Message> From<LoadingWheelWidget> for Element<'a, Message, iced::Theme, Renderer>
where
    Message: Clone + 'a,
{
    fn from(wheel: LoadingWheelWidget) -> Self {
        Self::new(wheel)
    }
}
