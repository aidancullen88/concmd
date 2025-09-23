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
use ratatui::widgets::{Block, Clear, List, ListState, Padding, Paragraph, Wrap};
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
    pub page_list: Vec<Page>,
    // Holds the ratatui list state (selected item) for each list
    pub space_list_state: ListState,
    pub page_list_state: ListState,
    // Tracks which area is active for keystrokes to apply to
    pub current_area: CurrentArea,
    pub exit: bool,
    // Used to transfer the edited file details between the edit and save updates
    pub edited_file_path: Option<PathBuf>,
    // Holds the saved states for edited pages for display in the pages list
    pub page_states_map: HashMap<String, PageState>,
    pub new_page_title: String,
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
            new_page_title: String::new(),
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
            // Nav keys don't do anything while the popup is active, so return
            _ => return,
        };
        if let Some(index) = list_state.selected() {
            if index >= list_length - 1 {
                // if we're at the end of the list then loop
                list_state.select_first();
            } else {
                list_state.select_next();
            }
            return;
        }
        // If nothing is selected, select the first item
        list_state.select_first();
    }

    pub fn list_previous(&mut self) {
        let list_state = match self.current_area {
            CurrentArea::Spaces => &mut self.space_list_state,
            CurrentArea::Pages => &mut self.page_list_state,
            // Nav keys don't do anything while the popup is active, so return
            _ => return,
        };
        if let Some(index) = list_state.selected() {
            if index == 0 {
                // If we're at the start of the list then loop
                list_state.select_last();
            } else {
                list_state.select_previous();
            }
            return;
        }
        // If nothing is selected, select the last item
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
    ListNext,
    ListPrevious,
    Select,
    Back,
    Exit,
    Save,
    ConfirmSave,
    RejectSave,
    Refresh,
    OpenEditor,
    NewPage,
    SaveNewPage,
    CancelNewPage,
    BackspaceNewPage,
    TypeNewPage(char),
    DeletePage,
    ConfirmDeletePage,
    CancelDeletePage,
}

// Possible states for an edited page to end up in
// Note that there is an implicit third option "not edited"
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
    NewPagePopup,
    DeletePopup,
}

// Entry point for the TUI
pub fn display(config: &Config) -> Result<()> {
    let mut terminal = ratatui::init();
    let spaces = actions::load_space_list(config)?;
    let mut app = App::new(spaces);
    // Store the result here so we can reset the terminal even if it's an error
    let result = run(config, &mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(config: &Config, terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.exit {
        terminal.draw(|frame| draw(frame, app))?;
        let mut message = handle_events(app)?;
        // Messages can chain other messages by returning a Some(Message)
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
    // NOTE: arms that do not return fall through and return Ok(None) i.e. do not chain another
    // update
    match message {
        Message::Exit => {
            // Reset the list state so that get_selected_page returns None while exiting
            app.page_list_state = ListState::default();
            app.exit = true;
        }
        Message::ListNext => {
            app.list_next();
        }
        Message::ListPrevious => {
            app.list_previous();
        }
        Message::Select => {
            match &app.current_area {
                CurrentArea::Spaces => {
                    // load page list and switch current_area
                    if let Some(selected_space) = app.get_selected_space() {
                        app.load_pages(config, &selected_space.id)?;
                        app.current_area = CurrentArea::Pages;
                    }
                }
                CurrentArea::Pages => return Ok(Some(Message::OpenEditor)),
                _ => {}
            }
        }
        Message::OpenEditor => {
            if let Some(mut page) = app.get_selected_page() {
                let edited_file_path = run_editor(terminal, config, &mut page)?;
                // Save the edited file path to use if the user wants to save
                app.edited_file_path = Some(edited_file_path);
                app.current_area = CurrentArea::SavePopup;
            }
        }
        Message::ConfirmSave => {
            if let CurrentArea::SavePopup = app.current_area {
                if let Some(page) = app.get_selected_page() {
                    // Save the page ID to the map to flag as saved in the UI
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
                    // Save the page ID to the map to flag as not saved in the UI
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
            _ => {}
        },
        Message::Refresh => app.refresh_current_list(config)?,
        // New page updates
        Message::NewPage => {
            app.current_area = CurrentArea::NewPagePopup;
        }
        Message::CancelNewPage => app.current_area = CurrentArea::Pages,
        Message::SaveNewPage => {
            actions::new_page_tui(
                config,
                &app.get_selected_space()
                    .expect("Should always be a space selected"),
                app.new_page_title.clone(),
            )?;
            app.current_area = CurrentArea::Pages;
            return Ok(Some(Message::Refresh));
        }
        Message::BackspaceNewPage => {
            app.new_page_title.pop();
        }
        Message::TypeNewPage(value) => app.new_page_title.push(value),
        Message::DeletePage => app.current_area = CurrentArea::DeletePopup,
        Message::ConfirmDeletePage => {
            if let Some(mut page) = app.get_selected_page() {
                actions::delete_page(&config.api, &mut page)?;
            };
            app.current_area = CurrentArea::Pages;
            return Ok(Some(Message::Refresh));
        }
        Message::CancelDeletePage => app.current_area = CurrentArea::Pages,
    }
    Ok(None)
}

fn draw(frame: &mut Frame, app: &mut App) {
    let main_title = Line::from("Concmd".bold());
    let instructions = match &app.current_area {
        CurrentArea::Spaces => Line::from("[r]efresh spaces "),
        CurrentArea::Pages => Line::from("[r]efresh pages | [n]ew page | [d]elete page "),
        _ => Line::from(""),
    };
    let container_block = Block::new()
        .title(main_title.centered())
        .title_bottom(instructions.right_aligned());
    let inner_area = container_block.inner(frame.area());
    frame.render_widget(container_block, frame.area());

    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Percentage(20),
            Constraint::Percentage(30),
            Constraint::Percentage(50),
        ])
        .split(inner_area);

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

        let page_marked_list = map_saved_pages(&app.page_list, &app.page_states_map);

        let page_list = List::new(page_marked_list).block(block).highlight_style(
            Style::default()
                .bg(ratatui::style::Color::LightYellow)
                .fg(ratatui::style::Color::Black),
        );
        frame.render_stateful_widget(page_list, layout[1], &mut app.page_list_state);
    }

    // Save popup block
    if let CurrentArea::SavePopup = app.current_area {
        let title = Line::from("Publish Page?".bold());
        let block = Block::bordered()
            .border_style(Style::new().yellow())
            .title(title.centered())
            .padding(Padding {
                left: 1,
                right: 1,
                top: 1,
                bottom: 1,
            });
        let question = Paragraph::new(Text::raw("Do you wish to save the edited page? [Y]es/[n]o"))
            .wrap(Wrap { trim: false })
            .block(block)
            .centered();
        let area = popup_area(frame.area(), 40, 6);
        frame.render_widget(Clear, area);
        frame.render_widget(question, area);
    }

    // New page title entry popup
    if let CurrentArea::NewPagePopup = app.current_area {
        let title = Line::from("Enter new page title".bold());
        let block = Block::bordered()
            .border_style(Style::new().yellow())
            .title(title.centered());
        let page_title = Paragraph::new(app.new_page_title.clone())
            .wrap(Wrap { trim: false })
            .block(block);
        let area = popup_area(frame.area(), 40, 5);
        frame.render_widget(Clear, area);
        frame.render_widget(page_title, area);
        frame.set_cursor_position((area.x + app.new_page_title.len() as u16 + 1, area.y + 1));
    }

    // Delete page confirmation popup
    if let CurrentArea::DeletePopup = app.current_area {
        let title = Line::from("Delete page?".bold());
        let block = Block::bordered()
            .border_style(Style::new().yellow())
            .title(title.centered())
            .padding(Padding {
                left: 1,
                right: 1,
                top: 1,
                bottom: 1,
            });
        let question = Paragraph::new(Text::raw(
            "Are you sure you want to delete this page? [Y]es/[n]o",
        ))
        .wrap(Wrap { trim: false })
        .block(block)
        .centered();
        let area = popup_area(frame.area(), 40, 6);
        frame.render_widget(Clear, area);
        frame.render_widget(question, area);
    }
}

// Helper functions

// Compute the area of the popup box
fn popup_area(area: Rect, max_x: u16, max_y: u16) -> Rect {
    let horizontal =
        Layout::horizontal([Constraint::Max(max_x)]).flex(ratatui::layout::Flex::Center);
    let vertical = Layout::vertical([Constraint::Max(max_y)]).flex(ratatui::layout::Flex::Center);
    let [area] = vertical.areas(area);
    let [area] = horizontal.areas(area);
    area
}

// Capture key events and return their message
fn handle_events(app: &App) -> Result<Option<Message>> {
    match event::read()? {
        Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
            Ok(handle_key_event(key_event, app))
        }
        _ => Ok(None),
    }
}

// Match a keycode to the correct message
fn handle_key_event(key_event: KeyEvent, app: &App) -> Option<Message> {
    // Universal events apply across all areas
    // match for more events in future instead of if let
    match key_event.code {
        KeyCode::Char('q') => return Some(Message::Exit),
        _ => {}
    }

    // Events for each area
    match app.current_area {
        CurrentArea::Spaces => match key_event.code {
            KeyCode::Up => Some(Message::ListPrevious),
            KeyCode::Down => Some(Message::ListNext),
            KeyCode::Left => Some(Message::Back),
            KeyCode::Right | KeyCode::Enter => Some(Message::Select),
            KeyCode::Char('r') => Some(Message::Refresh),
            _ => None,
        },
        CurrentArea::Pages => match key_event.code {
            KeyCode::Up => Some(Message::ListPrevious),
            KeyCode::Down => Some(Message::ListNext),
            KeyCode::Left => Some(Message::Back),
            KeyCode::Right | KeyCode::Enter => Some(Message::Select),
            KeyCode::Char('r') => Some(Message::Refresh),
            KeyCode::Char('n') => Some(Message::NewPage),
            KeyCode::Char('d') => Some(Message::DeletePage),
            _ => None,
        },
        CurrentArea::SavePopup => match key_event.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(Message::ConfirmSave),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(Message::RejectSave),
            _ => None,
        },
        CurrentArea::NewPagePopup => match key_event.code {
            KeyCode::Enter => Some(Message::SaveNewPage),
            KeyCode::Esc => Some(Message::CancelNewPage),
            KeyCode::Backspace => Some(Message::BackspaceNewPage),
            KeyCode::Char(value) => Some(Message::TypeNewPage(value)),
            _ => None,
        },
        CurrentArea::DeletePopup => match key_event.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(Message::ConfirmDeletePage),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(Message::CancelDeletePage),
            _ => None,
        },
    }
}

// Pass terminal control to the editor correctly and then take it back once it exits
fn run_editor(terminal: &mut DefaultTerminal, config: &Config, page: &mut Page) -> Result<PathBuf> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    let file_path = actions::edit_page(config, page)?;
    stdout().execute(EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(file_path)
}

// Anything that implements Named can be turned into a list of names for the ui
fn get_name_list<N: Named>(item_list: &[N]) -> Vec<String> {
    item_list.iter().map(|i| i.get_name()).collect()
}

// Maps a list of pages to their names + their status by looking up their IDs in the states hash
fn map_saved_pages(item_list: &[Page], states_hash: &HashMap<String, PageState>) -> Vec<String> {
    item_list
        .iter()
        .map(|i| {
            if let Some(page_id) = &i.id {
                match states_hash.get(page_id) {
                    Some(PageState::Saved) => format!("{} {}", "✓", i.get_name()),
                    Some(PageState::NotSaved) => format!("{} {}", "✕", i.get_name()),
                    None => format!("  {}", i.get_name()),
                }
            // Else branch should never be hit but is required by the complier so implemented
            // anyway
            } else {
                format!("  {}", i.get_name())
            }
        })
        .collect()
}
