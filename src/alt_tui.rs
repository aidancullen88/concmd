use std::collections::HashMap;
use std::io::stdout;
use std::path::PathBuf;

use crate::conf_api::{Named, Page, Space};
use crate::{actions, Config};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::crossterm::ExecutableCommand;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::symbols::border;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Clear, List, ListState, Paragraph};
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
    pub current_area: CurrentArea,
    pub exit: bool,
    pub edited_file_path: Option<PathBuf>,
    pub page_states_map: HashMap<String, PageState>,
}

impl App {
    pub fn new(space_list: Vec<Space>) -> App {
        App {
            space_list,
            space_list_state: ListState::default(),
            page_list: vec![],
            page_list_state: ListState::default(),
            current_area: CurrentArea::Spaces,
            exit: false,
            edited_file_path: None,
            page_states_map: HashMap::new(),
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
        let (list_state, list_length) = match self.current_area {
            CurrentArea::Spaces => {
                let list_length = self.space_list.len();
                (&mut self.space_list_state, list_length)
            }
            CurrentArea::Pages => {
                let list_length = self.page_list.len();
                (&mut self.page_list_state, list_length)
            }
            CurrentArea::SavePopup => return,
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
        let list_state = match self.current_area {
            CurrentArea::Spaces => &mut self.space_list_state,
            CurrentArea::Pages => &mut self.page_list_state,
            CurrentArea::SavePopup => return,
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

    pub fn refresh_current_list(&mut self, config: &Config) -> Result<()> {
        match self.current_area {
            CurrentArea::Pages => self.load_pages(
                config,
                &self
                    .get_selected_space()
                    .expect("If we're in the pages pane there must be a selected space")
                    .id,
            ),
            CurrentArea::Spaces => {
                self.space_list = actions::load_space_list(config)?;
                Ok(())
            }
            _ => Ok(()),
        }
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
    ConfirmSave,
    RejectSave,
    Refresh,
    OpenEditor,
}

#[derive(Clone, Debug)]
pub enum PageState {
    NotSaved,
    Saved,
}

// Represents the current list the user is selecting
#[derive(Clone, Debug)]
pub enum CurrentArea {
    Spaces,
    Pages,
    SavePopup,
}

// Entry point for the TUI
pub fn display(config: &Config) -> Result<()> {
    // let _ = simple_logging::log_to_file("next_handler.log", log::LevelFilter::Info);
    let mut terminal = ratatui::init();
    let spaces = actions::load_space_list(config)?;
    let mut app = App::new(spaces);
    // If the user exits without saving, the selected page is cleared and app.get_selected_page
    // will return None. We can rely on this to check if the edit flow should continue or not.
    let result = run(config, &mut terminal, &mut app);
    // Needs to always run to hand back control to the terminal properly, so it lives here above
    // the match
    ratatui::restore();
    result
}

fn run(config: &Config, terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.exit {
        terminal.draw(|frame| draw(frame, app))?;
        let mut message = handle_events()?;
        // Messages can chain other messages (see Message::Select in update)
        while message.is_some() {
            message = update(app, config, message.unwrap(), terminal)?;
        }
    }
    Ok(())
}

fn update(
    app: &mut App,
    config: &Config,
    message: Message,
    terminal: &mut DefaultTerminal,
) -> Result<Option<Message>> {
    match message {
        Message::Exit => {
            // Reset the list state so that get_selected_page returns None while exiting
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
            match &app.current_area {
                CurrentArea::Spaces => {
                    // load page list and switch current_pane
                    if let Some(selected_space) = app.get_selected_space() {
                        app.load_pages(config, &selected_space.id)?;
                        app.current_area = CurrentArea::Pages;
                    }
                }
                CurrentArea::Pages => return Ok(Some(Message::OpenEditor)),
                _ => return Ok(None),
            }
        }
        Message::OpenEditor => {
            if let Some(mut page) = app.get_selected_page() {
                let edited_file_path = run_editor(terminal, config, &mut page)?;
                app.edited_file_path = Some(edited_file_path);
                app.current_area = CurrentArea::SavePopup;
            } else {
                bail!("Editor attempted to open without page selected")
            }
        }
        Message::ConfirmSave => {
            if let CurrentArea::SavePopup = app.current_area {
                if let Some(page) = app.get_selected_page() {
                    app.page_states_map.insert(
                        page.id.expect("Page from API should always have an ID"),
                        PageState::Saved,
                    );
                    return Ok(Some(Message::Save));
                }
                bail!("Attempted to save without a page selected")
            }
        }
        Message::RejectSave => {
            if let CurrentArea::SavePopup = app.current_area {
                if let Some(page) = app.get_selected_page() {
                    app.current_area = CurrentArea::Pages;
                    app.page_states_map.insert(
                        page.id.expect("Page from API should always have an ID"),
                        PageState::NotSaved,
                    );
                }
            }
        }
        Message::Save => {
            if let CurrentArea::SavePopup = app.current_area {
                if let Some(mut page) = app.get_selected_page() {
                    actions::upload_edited_page(config, &mut page, app.edited_file_path.as_ref())?;
                    app.current_area = CurrentArea::Pages;
                    // Refresh the page list so that pages can be edited again
                    return Ok(Some(Message::Refresh));
                } else {
                    bail!("Attempted to save without a selected page")
                }
            }
        }
        Message::Back => match &app.current_area {
            CurrentArea::Pages => {
                // Clear out the pages list and reset the state
                app.page_list = vec![];
                app.page_list_state = ListState::default();
                app.current_area = CurrentArea::Spaces;
            }
            CurrentArea::Spaces => {
                app.space_list_state = ListState::default();
            }
            _ => return Ok(None),
        },
        Message::Refresh => app.refresh_current_list(config)?,
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

    let space_list = List::new(get_name_list(&app.space_list))
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

        let page_marked_list = map_saved_names(&app.page_list, &app.page_states_map);

        let page_list = List::new(page_marked_list).block(block).highlight_style(
            Style::default()
                .bg(ratatui::style::Color::LightYellow)
                .fg(ratatui::style::Color::Black),
        );
        frame.render_stateful_widget(page_list, layout[1], &mut app.page_list_state);
    }

    if let CurrentArea::SavePopup = app.current_area {
        let title = Line::from("Publish Page?".bold());
        let block = Block::bordered()
            .border_style(Style::new().yellow())
            .title(title.centered());
        let question = Paragraph::new(Text::raw("\n[Y]es/[n]o"))
            .block(block)
            .centered();
        let area = popup_area(frame.area(), 20, 5);
        frame.render_widget(Clear, area);
        frame.render_widget(question, area);
    }
}

fn popup_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
    let horizontal =
        Layout::horizontal([Constraint::Max(percent_x)]).flex(ratatui::layout::Flex::Center);
    let vertical =
        Layout::vertical([Constraint::Max(percent_y)]).flex(ratatui::layout::Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
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
        KeyCode::Up => Some(Message::Previous),
        KeyCode::Down => Some(Message::Next),
        KeyCode::Left => Some(Message::Back),
        KeyCode::Right | KeyCode::Enter => Some(Message::Select),
        KeyCode::Char('r') | KeyCode::F(5) => Some(Message::Refresh),
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(Message::ConfirmSave),
        KeyCode::Char('n') | KeyCode::Char('N') => Some(Message::RejectSave),
        _ => None,
    }
}

fn run_editor(terminal: &mut DefaultTerminal, config: &Config, page: &mut Page) -> Result<PathBuf> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    let file_path = actions::edit_page(config, page)?;
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(file_path)
}

fn get_name_list<N: Named>(item_list: &[N]) -> Vec<String> {
    item_list.iter().map(|i| i.get_name()).collect()
}

fn map_saved_names(item_list: &[Page], states_hash: &HashMap<String, PageState>) -> Vec<String> {
    item_list
        .iter()
        .map(|i| {
            if let Some(page_id) = &i.id {
                match states_hash.get(page_id) {
                    Some(PageState::Saved) => format!("{} {}", "✓", i.get_name()),
                    Some(PageState::NotSaved) => format!("{} {}", "✕", i.get_name()),
                    None => format!("  {}", i.get_name()),
                }
            } else {
                format!("  {}", i.get_name())
            }
        })
        .collect()
}
