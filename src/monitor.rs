use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::{StreamExt};
use ratatui::buffer::Buffer;
use ratatui::layout::{
    Constraint::{Length, Min, Ratio},
    Layout, Rect,
};
use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Padding, Widget};
use ratatui::DefaultTerminal;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::watch;
const CUSTOM_LABEL_COLOR: Color = tailwind::SLATE.c200;
const GAUGE_COLOR: Color = tailwind::GREEN.c800;

pub struct Monitor {
    terminal: DefaultTerminal,
    rx: UnboundedReceiver<bool>,
    inner_tx: watch::Sender<bool>,
    inner_rx: watch::Receiver<bool>,
    progress: f64,
}

impl Monitor {
    const FRAMES_PER_SECOND: f32 = 60.0;

    pub fn new(rx: UnboundedReceiver<bool>) -> Self {
        let (inner_tx, inner_rx) = watch::channel(false);
        let terminal = ratatui::init();
        Monitor {
            terminal,
            rx,
            inner_rx,
            inner_tx,
            progress: 10.0,
        }
    }

    pub fn draw(&mut self) {
        let widget = MonitorWidget {
            progress: self.progress,
        };
        self.terminal
            .draw(|frame| {
                frame.render_widget(widget, frame.area());
            })
            .unwrap();
    }
    pub fn get_receiver(&self) -> watch::Receiver<bool> {
        self.inner_rx.clone()
    }

    pub async fn run(&mut self) {
        self.terminal.clear().unwrap();
        let mut events = EventStream::new();
        loop {
            let period = Duration::from_secs_f32(1.0 / Self::FRAMES_PER_SECOND);
            let mut interval = tokio::time::interval(period);
            let tx = self.inner_tx.clone();
            tokio::select! {
                 _ = interval.tick() => {
                    self.draw();

                    self.progress += 1.0;
                    if self.progress > 100.0 {
                        self.progress = 100.0;
                    }
                }
                _ = self.rx.recv() => {
                    break;
                }
                Some(Ok(event)) = events.next() => {
                    if let Some(_) = self.handle_event(&event) {
                        tx.send(true).unwrap();
                        break;
                    }
                }
            }
        }
        ratatui::restore();
    }
    fn handle_event(&mut self, event: &Event) -> Option<bool> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => Some(true),
            _ => None,
        }
    }
}

struct MonitorWidget {
    progress: f64,
}

impl MonitorWidget {
    pub fn render_title<'a>(&self, title: &'a str) -> Block<'a> {
        let title = Line::from(title).centered();
        Block::default()
            .borders(Borders::NONE)
            .padding(Padding::vertical(1))
            .title(title)
            .style(Style::default().fg(CUSTOM_LABEL_COLOR))
    }
    pub fn render_gauge<'a>(&self, title: Block<'a>) -> Gauge<'a> {
        let label = Span::styled(
            format!("{:.1}/100", self.progress),
            Style::default().italic().bold().fg(CUSTOM_LABEL_COLOR),
        );
        Gauge::default()
            .block(title)
            .gauge_style(Style::default().fg(GAUGE_COLOR))
            .ratio(self.progress / 100.0)
            .label(label)
    }
}

impl Widget for MonitorWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Length(2), Min(0), Length(1)].as_ref())
            .split(area);
        let gauge_area = layout[1];

        let gauge_layout = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Ratio(1, 4)].as_ref())
            .split(gauge_area);
        let gauge_area = gauge_layout[0];

        let title = self.render_title("Progress Bar");

        let gauge = self.render_gauge(title);
        gauge.render(gauge_area, buf);
    }
}
