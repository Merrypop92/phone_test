use esp32_quick::Epd;
use heapless::String as HeapString;

pub const MAX_TASKS: usize = 8;
pub const MAX_TITLE_BYTES: usize = 48;

pub struct Task {
    pub title: HeapString<MAX_TITLE_BYTES>,
    pub done: bool,
}

pub enum AppCommand {
    Stay,
    BackToMenu,
    OpenMessages,
    OpenTasks,
    OpenCalculator,
}

pub trait App {
    fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]);
    fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand;
}

#[derive(Clone, Copy)]
pub struct MenuController {
    pub items: [&'static str; 3],
    pub selected: usize,
}

impl MenuController {
    pub fn new() -> Self {
        Self {
            items: ["Messages", "Taches", "Calculatrice"],
            selected: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected == 0 {
            self.selected = self.items.len() - 1;
        } else {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        self.selected = (self.selected + 1) % self.items.len();
    }

    pub fn selected_app(&self) -> AppCommand {
        match self.selected {
            0 => AppCommand::OpenMessages,
            1 => AppCommand::OpenTasks,
            _ => AppCommand::OpenCalculator,
        }
    }
}

pub enum ActiveApp {
    Menu(super::menu_app::MenuApp),
    Messages(super::message_app::MessageApp),
    Tasks(super::task_app::TaskApp),
    Calculator(super::calculator_app::CalculatorApp),
}

impl ActiveApp {
    pub fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        match self {
            ActiveApp::Menu(app) => app.draw(epd, buf),
            ActiveApp::Messages(app) => app.draw(epd, buf),
            ActiveApp::Tasks(app) => app.draw(epd, buf),
            ActiveApp::Calculator(app) => app.draw(epd, buf),
        }
    }

    pub fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        match self {
            ActiveApp::Menu(app) => app.handle_input(input, epd, buf),
            ActiveApp::Messages(app) => app.handle_input(input, epd, buf),
            ActiveApp::Tasks(app) => app.handle_input(input, epd, buf),
            ActiveApp::Calculator(app) => app.handle_input(input, epd, buf),
        }
    }
}
