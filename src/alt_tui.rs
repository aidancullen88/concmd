use crate::conf_api::{Name, Page, Space};
use crate::{actions, Config};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::symbols::border;
use ratatui::text::Line;
use ratatui::widgets::{Block, List, ListState};
use ratatui::DefaultTerminal;
use ratatui::Frame;

use anyhow::{bail, Result};

/* Concmd uses the ELM architecture:
* draw the UI based on the state
* parse input events into a message
* update the state based on the message
* loop
*/

// Holds the entire state of the app
pub struct App {
    pub space_list: Vec<Space>,
    pub space_list_state: ListState,
    pub page_list: Vec<Page>,
    pub page_list_state: ListState,
    pub current_pane: CurrentPane,
    pub exit: bool,
}

impl App {
    pub fn new(space_list: Vec<Space>) -> App {
        App {
            space_list,
            space_list_state: ListState::default(),
            page_list: vec![],
            page_list_state: ListState::default(),
            current_pane: CurrentPane::Spaces,
            exit: false,
        }
    }

    pub fn load_pages(&mut self, config: &Config, space_id: &str) -> Result<()> {
        self.page_list = actions::load_page_list_for_space(config, space_id)?;
        Ok(())
    }

    // Gets the currently selected space based on the app state
    // Combines the space list and the space list state
    pub fn get_selected_space(&self) -> Option<Space> {
        if let Some(selected_index) = self.space_list_state.selected() {
            return self.space_list.get(selected_index).cloned();
        }
        None
    }

    // Gets the currently selected page based on the app state
    // Combines the page list and page list state
    // This is much more likely to return None than the space function above
    // as there is often no page selected (when changing space options for instance)
    pub fn get_selected_page(&self) -> Option<Page> {
        if let Some(selected_index) = self.page_list_state.selected() {
            return self.page_list.get(selected_index).cloned();
        }
        None
    }

    // Helper functions that enable both lists to be manipulated without duplicate calls
    // Also handle list wrapping
    pub fn list_next(&mut self) {
        let (list_state, list_length) = match self.current_pane {
            CurrentPane::Spaces => {
                let list_length = self.space_list.len();
                (&mut self.space_list_state, list_length)
            }
            CurrentPane::Pages => {
                let list_length = self.page_list.len();
                (&mut self.page_list_state, list_length)
            }
        };
        if let Some(index) = list_state.selected() {
            if index >= list_length - 1 {
                list_state.select_first();
            } else {
                list_state.select_next();
            }
            return;
        }
        list_state.select_first();
    }

    pub fn list_previous(&mut self) {
        let list_state = match self.current_pane {
            CurrentPane::Spaces => &mut self.space_list_state,
            CurrentPane::Pages => &mut self.page_list_state,
        };
        if let Some(index) = list_state.selected() {
            if index == 0 {
                list_state.select_last();
            } else {
                list_state.select_previous();
            }
            return;
        }
        list_state.select_last();
    }
}

// Represents all possible user actions in the app
enum Message {
    Next,
    Previous,
    Select,
    Back,
    Exit,
    Save,
}

// Represents the current list the user is selecting
#[derive(Clone, Debug)]
pub enum CurrentPane {
    Spaces,
    Pages,
}

// Entry point for the TUI
pub fn display(config: &Config) -> Result<Page> {
    // let _ = simple_logging::log_to_file("next_handler.log", log::LevelFilter::Info);
    let mut terminal = ratatui::init();
    let spaces = actions::load_space_list(config)?;
    let mut app = App::new(spaces);
    // If the user exits without saving, the selected page is cleared and app.get_selected_page
    // will return None. We can rely on this to check if the edit flow should continue or not.
    let app_result = run(config, &mut terminal, &mut app);
    // Needs to always run to hand back control to the terminal properly, so it lives here above
    // the match
    ratatui::restore();
    match app_result {
        Ok(_) => {
            if let Some(page) = app.get_selected_page() {
                Ok(page)
            } else {
                bail!("USER_APP_EXIT")
            }
        }
        Err(e) => Err(e),
    }
}

fn run(config: &Config, terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.exit {
        terminal.draw(|frame| draw(frame, app))?;
        let mut message = handle_events()?;
        // Messages can chain other messages (see Message::Select in update)
        while message.is_some() {
            message = update(app, config, message.unwrap())?;
        }
    }
    Ok(())
}

fn update(app: &mut App, config: &Config, message: Message) -> Result<Option<Message>> {
    match message {
        Message::Exit => {
            app.page_list_state = ListState::default();
            app.exit = true;
        }
        Message::Next => {
            app.list_next();
        }
        Message::Previous => {
            app.list_previous();
        }
        Message::Select => {
            match &app.current_pane {
                CurrentPane::Spaces => {
                    // load page list and switch current_pane
                    if let Some(selected_space) = app.get_selected_space() {
                        app.load_pages(config, &selected_space.id)?;
                        app.current_pane = CurrentPane::Pages;
                    }
                }
                CurrentPane::Pages => return Ok(Some(Message::Save)),
            }
        }
        Message::Save => app.exit = true,
        Message::Back => match &app.current_pane {
            CurrentPane::Pages => {
                app.page_list = vec![];
                app.page_list_state = ListState::default();
                app.current_pane = CurrentPane::Spaces;
            }
            CurrentPane::Spaces => {
                app.space_list_state = ListState::default();
            }
        },
    }
    Ok(None)
}

fn draw(frame: &mut Frame, app: &mut App) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Percentage(20),
            Constraint::Percentage(30),
            Constraint::Percentage(50),
        ])
        .split(frame.area());

    // Space list block
    let title = Line::from("Spaces".bold());

    let block = Block::bordered()
        .title(title.centered())
        .border_set(border::THICK);

    let space_list = List::new(get_name_list(app.space_list.clone()))
        .block(block)
        .highlight_style(
            Style::default()
                .bg(ratatui::style::Color::LightYellow)
                .fg(ratatui::style::Color::Black),
        );

    frame.render_stateful_widget(space_list, layout[0], &mut app.space_list_state);

    // Page list block
    if !app.page_list.is_empty() {
        let title = Line::from("Pages".bold());

        let block = Block::bordered()
            .title(title.centered())
            .border_set(border::THICK);

        let page_list = List::new(get_name_list(app.page_list.clone()))
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(ratatui::style::Color::LightYellow)
                    .fg(ratatui::style::Color::Black),
            );
        frame.render_stateful_widget(page_list, layout[1], &mut app.page_list_state);
    }
}

fn handle_events() -> Result<Option<Message>> {
    match event::read()? {
        Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
            Ok(handle_key_event(key_event))
        }
        _ => Ok(None),
    }
}

fn handle_key_event(key_event: KeyEvent) -> Option<Message> {
    match key_event.code {
        KeyCode::Char('q') => Some(Message::Exit),
        KeyCode::Down => Some(Message::Next),
        KeyCode::Up => Some(Message::Previous),
        KeyCode::Enter => Some(Message::Select),
        KeyCode::Right => Some(Message::Select),
        KeyCode::Left => Some(Message::Back),
        _ => None,
    }
}

fn get_name_list<N: Name>(item_list: Vec<N>) -> Vec<String> {
    item_list.iter().map(|i| i.get_name()).collect()
}
