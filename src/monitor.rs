use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use futures::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::layout::{
    Constraint::{Length, Percentage},
    Layout, Rect,
};
use ratatui::style::palette::tailwind;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Gauge, Padding, Paragraph, Widget};
use ratatui::DefaultTerminal;
use std::error::Error;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::watch;
const CUSTOM_LABEL_COLOR: Color = tailwind::SLATE.c200;
const GAUGE_COLOR: Color = tailwind::GREEN.c800;

#[derive(Debug, Clone)]
pub struct Message {
    pub duration: u64,
    pub is_success: bool,
    pub average_duration: u64,
}

pub struct Monitor {
    now: Instant,
    terminal: DefaultTerminal,
    rx: UnboundedReceiver<Option<Message>>,
    inner_tx: watch::Sender<bool>,
    inner_rx: watch::Receiver<bool>,
    widget: MonitorWidget,
}

impl Monitor {
    const FRAMES_PER_SECOND: f32 = 60.0;

    pub fn new(
        now: Instant,
        rx: UnboundedReceiver<Option<Message>>,
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

    pub fn draw(&mut self) -> Result<(), Box<dyn Error>> {
        let widget = self.widget.clone();
        self.terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(widget, area);
        })?;
        Ok(())
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
                    self.draw().unwrap_or_else(|_| {});
                }
                // channel
                Some(x) = self.rx.recv() => {
                    let Some(msg) = x else {
                        break;
                    };
                    self.widget.count += 1;
                    self.widget.average_time = msg.average_duration;
                    if msg.is_success {
                        self.widget.success_count += 1;
                    }
                    if msg.duration > self.widget.max_time {
                        self.widget.max_time = msg.duration;
                    }
                    if msg.duration < self.widget.min_time || self.widget.min_time == 0 {
                        self.widget.min_time = msg.duration;
                    }
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
    max_time: u64,
    min_time: u64,
    average_time: u64,
    success_count: u64,
    count: u64,
    seconds: u64,
    max_count: u64,
    duration: Option<u64>,
}

impl MonitorWidget {
    pub fn new(seconds: u64, max_count: u64, duration: Option<u64>) -> Self {
        MonitorWidget {
            max_time: 0,
            min_time: 0,
            average_time: 0,
            success_count: 0,
            count: 0,
            seconds,
            max_count,
            duration,
        }
    }
    pub fn get_gauge<'a>(&self) -> Gauge<'a> {
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
            .gauge_style(Style::default().fg(GAUGE_COLOR))
            .ratio(ratio)
            .label(label)
    }
    pub fn get_success_rate<'a>(&self) -> Paragraph<'a> {
        let rate = if self.count == 0 {
            0.0
        } else {
            self.success_count as f64 / self.count as f64 * 100.0
        };
        let text = format!(
            "Success: {}/{} Success Rate: {:.2}%",
            self.success_count, self.count, rate
        );
        Paragraph::new(text)
    }
    pub fn get_time_info<'a>(&self) -> Paragraph<'a> {
        let text = format!(
            "Max Time: {} Min Time: {} Average Time: {}",
            self.max_time, self.min_time, self.average_time
        );
        Paragraph::new(text)
    }
    pub fn get_layout(&self) -> [Layout; 2] {
        let outer = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([Length(6), Length(6)]);
        let inner = Layout::default()
            .direction(ratatui::layout::Direction::Horizontal)
            .constraints([Percentage(50), Percentage(50)]);
        [outer, inner]
    }
    pub fn get_box<'a>(&self, title: &'a str) -> Block<'a> {
        let outer_block = Block::bordered()
            .border_type(BorderType::Thick)
            .title(Line::from(title).centered())
            .padding(Padding::horizontal(1));
        outer_block
    }
}

impl Widget for MonitorWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let [outer_layout, inner_layout] = self.get_layout();
        let rects = outer_layout.split(area);
        let row = rects[0];
        let row_2_left = inner_layout.split(rects[1])[0];
        let row_2_right = inner_layout.split(rects[1])[1];

        let gauge = self.get_gauge();
        let b_box = self.get_box("Progress Bar");
        let inner = b_box.inner(row);

        let success_rate_text = self.get_success_rate();
        let b_box2 = self.get_box("Success Rate");
        let inner2 = b_box.inner(row_2_left);

        let time_text = self.get_time_info();
        let b_box3 = self.get_box("Time Elapsed");
        let inner3 = b_box.inner(row_2_right);

        gauge.render(inner, buf);
        success_rate_text.render(inner2, buf);
        time_text.render(inner3, buf);

        b_box.render(row, buf);
        b_box2.render(row_2_left, buf);
        b_box3.render(row_2_right, buf);
    }
}
