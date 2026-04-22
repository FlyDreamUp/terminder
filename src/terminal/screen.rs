use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi;

pub struct TerminalScreen {
    term: Term<VoidListener>,
    parser: ansi::Processor,
    cols: u16,
    rows: u16,
}

impl TerminalScreen {
    pub fn new(rows: u16, cols: u16, scrollback: usize) -> Self {
        let mut config = Config::default();
        config.scrolling_history = scrollback;
        let dimensions = ScreenDimensions::new(rows, cols);
        Self {
            term: Term::new(config, &dimensions, VoidListener),
            parser: ansi::Processor::new(),
            cols,
            rows,
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 || (self.cols == cols && self.rows == rows) {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.resize(ScreenDimensions::new(rows, cols));
    }

    pub fn contents(&self) -> String {
        if self.cols == 0 || self.rows == 0 {
            return String::new();
        }

        let start = Point::new(self.term.topmost_line(), Column(0));
        let end = Point::new(self.term.bottommost_line(), self.term.last_column());
        self.term.bounds_to_string(start, end)
    }
}

#[derive(Clone, Copy)]
struct ScreenDimensions {
    rows: usize,
    cols: usize,
}

impl ScreenDimensions {
    fn new(rows: u16, cols: u16) -> Self {
        Self {
            rows: rows as usize,
            cols: cols as usize,
        }
    }
}

impl Dimensions for ScreenDimensions {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}
