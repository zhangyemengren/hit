use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::layout::{
    Constraint::Length,
    Layout, Rect,
};
use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Padding, Widget};
use ratatui::DefaultTerminal;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::watch;
const CUSTOM_LABEL_COLOR: Color = tailwind::SLATE.c200;
const GAUGE_COLOR: Color = tailwind::GREEN.c800;

pub struct Monitor {
    now: Instant,
    terminal: DefaultTerminal,
    rx: UnboundedReceiver<Option<u64>>,
    inner_tx: watch::Sender<bool>,
    inner_rx: watch::Receiver<bool>,
    widget: MonitorWidget,
}

impl Monitor {
    const FRAMES_PER_SECOND: f32 = 60.0;

    pub fn new(
        now: Instant,
        rx: UnboundedReceiver<Option<u64>>,
        max_count: u64,
        duration: Option<u64>,
    ) -> Self {
        let (inner_tx, inner_rx) = watch::channel(false);
        let terminal = ratatui::init();
        let widget = MonitorWidget::new(0, max_count, duration);
        Monitor {
            now,
            terminal,
            rx,
            inner_rx,
            inner_tx,
            widget,
        }
    }

    pub fn draw(&mut self) {
        let widget = self.widget.clone();
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
        let mut tick_now = self.now.clone();
        loop {
            let period = Duration::from_secs_f32(1.0 / Self::FRAMES_PER_SECOND);
            let mut interval = tokio::time::interval(period);
            let tx = self.inner_tx.clone();
            tokio::select! {
                 // 如果
                 _ = interval.tick() => {
                    if tick_now.elapsed().as_secs() >= 1 {
                        tick_now = Instant::now();
                        self.widget.seconds += 1;
                    }
                    self.draw();
                }
                // channel
                Some(x) = self.rx.recv() => {
                    if let None = x{
                        break;
                    }
                    self.widget.count += 1;
                }
                // 事件
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

#[derive(Clone)]
struct MonitorWidget {
    count: u64,
    seconds: u64,
    max_count: u64,
    duration: Option<u64>,
}

impl MonitorWidget {
    pub fn new(seconds: u64, max_count: u64, duration: Option<u64>) -> Self {
        MonitorWidget {
            count: 0,
            seconds,
            max_count,
            duration,
        }
    }
    pub fn render_title<'a>(&self, title: &'a str) -> Block<'a> {
        let title = Line::from(title).centered();
        Block::default()
            .borders(Borders::NONE)
            .padding(Padding::vertical(1))
            .title(title)
            .style(Style::default().fg(CUSTOM_LABEL_COLOR))
    }
    pub fn render_gauge<'a>(&self, title: Block<'a>) -> Gauge<'a> {
        let (text, mut ratio) = match self.duration {
            Some(duration) => (
                format!("{}/{}", self.seconds, duration),
                self.seconds as f64 / duration as f64,
            ),
            None => (
                format!("{}/{}", self.count, self.max_count),
                self.count as f64 / self.max_count as f64,
            ),
        };
        let label = Span::styled(
            text,
            Style::default().italic().bold().fg(CUSTOM_LABEL_COLOR),
        );
        if ratio > 1.0 {
            ratio = 1.0;
        }
        Gauge::default()
            .block(title)
            .gauge_style(Style::default().fg(GAUGE_COLOR))
            .ratio(ratio)
            .label(label)
    }
}

impl Widget for MonitorWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Length(6)])
            .split(area);
        let gauge_area = layout[0];

        let title = self.render_title("Progress Bar");
        let gauge = self.render_gauge(title);

        gauge.render(gauge_area, buf);
    }
}
