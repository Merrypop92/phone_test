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

enum ActiveScreen {
    Menu,
    Messages,
    Tasks,
    Calculator,
}

pub struct ActiveApp {
    screen: ActiveScreen,
    menu: super::menu_app::MenuApp,
    messages: super::message_app::MessageApp,
    tasks: super::task_app::TaskApp,
    calculator: super::calculator_app::CalculatorApp,
}

impl ActiveApp {
    pub fn new() -> Self {
        Self {
            screen: ActiveScreen::Menu,
            menu: super::menu_app::MenuApp::new(),
            messages: super::message_app::MessageApp::new(),
            tasks: super::task_app::TaskApp::new(),
            calculator: super::calculator_app::CalculatorApp::new(),
        }
    }

    pub fn draw(&mut self, epd: &mut Epd, buf: &mut [u8]) {
        match self.screen {
            ActiveScreen::Menu => self.menu.draw(epd, buf),
            ActiveScreen::Messages => self.messages.draw(epd, buf),
            ActiveScreen::Tasks => self.tasks.draw(epd, buf),
            ActiveScreen::Calculator => self.calculator.draw(epd, buf),
        }
    }

    pub fn handle_input(&mut self, input: &str, epd: &mut Epd, buf: &mut [u8]) -> AppCommand {
        match self.screen {
            ActiveScreen::Menu => self.menu.handle_input(input, epd, buf),
            ActiveScreen::Messages => self.messages.handle_input(input, epd, buf),
            ActiveScreen::Tasks => self.tasks.handle_input(input, epd, buf),
            ActiveScreen::Calculator => self.calculator.handle_input(input, epd, buf),
        }
    }

    pub fn activate(&mut self, command: AppCommand, epd: &mut Epd, buf: &mut [u8]) {
        self.screen = match command {
            AppCommand::Stay => return,
            AppCommand::BackToMenu => ActiveScreen::Menu,
            AppCommand::OpenMessages => ActiveScreen::Messages,
            AppCommand::OpenTasks => ActiveScreen::Tasks,
            AppCommand::OpenCalculator => ActiveScreen::Calculator,
        };
        self.draw(epd, buf);
    }
}
