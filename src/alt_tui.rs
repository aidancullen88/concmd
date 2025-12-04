use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;
use std::io::stdout;
use std::iter::zip;
use std::path::PathBuf;

use crate::conf_api::{Attr, Page, Space};
use crate::{Config, actions};

// use crossterm::event::{
//     self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEvent,
//     MouseEventKind,
// };
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::ExecutableCommand;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::symbols::border;
use ratatui::text::{Line, Text};
use ratatui::widgets::block::Title;
use ratatui::widgets::{Block, Clear, List, ListState, Padding, Paragraph, Wrap};

use anyhow::Result;

/* Concmd uses the ELM architecture:
* draw the UI based on the state
* parse input events into a message
* update the state based on the message
* loop
*/

// Holds the entire state of the app
struct App {
    space_list: Vec<Space>,
    page_list: Vec<Page>,
    // Holds the ratatui list state (selected item) for each list
    space_list_state: ListState,
    page_list_state: ListState,
    // Tracks which area is active for keystrokes to apply to
    current_area: CurrentArea,
    exit: bool,
    // Used to transfer the edited file details between the edit and save updates
    edited_file_path: Option<PathBuf>,
    // Holds the saved states for edited pages for display in the pages list
    page_states_map: HashMap<String, PageState>,
    new_page_title: String,
    // "Universal" cursor for text entry fields. Make sure to clear after using!
    cursor_negative_offset: usize,
    search: Search,
    // Toggles for keybinds to turn features on and off
    show_preview: bool,
    show_help: bool,
    sort: Sort,
    space_list_pos: Bounds,
    page_list_pos: Bounds,
    page_updated_title: String,
}

#[derive(Default)]
struct Bounds {
    left: u16,
    right: u16,
    top: u16,
    // bottom: u16,
}

struct Search {
    current_search: String,
    search_active: bool,
}

struct Sort {
    type_state: ListState,
    dir_state: SortDirection,
    sort_types_array: [SortType; 2],
    saved_state: (ListState, SortDirection),
}

#[derive(Clone, Copy)]
enum SortType {
    Title,
    CreatedOn,
}

impl fmt::Display for SortType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SortType::Title => write!(f, "Title"),
            SortType::CreatedOn => write!(f, "Created Date"),
        }
    }
}

#[derive(Clone, Copy)]
enum SortDirection {
    Asc,
    Desc,
}

impl fmt::Display for SortDirection {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SortDirection::Asc => write!(f, "Asc"),
            SortDirection::Desc => write!(f, "Desc"),
        }
    }
}

impl App {
    fn new(space_list: Vec<Space>) -> App {
        App {
            space_list,
            space_list_state: ListState::default(),
            // Empty list displays the same as None, and we don't have to unwrap the option every
            // time we check the list
            page_list: vec![],
            page_list_state: ListState::default(),
            current_area: CurrentArea::Spaces,
            exit: false,
            edited_file_path: None,
            page_states_map: HashMap::new(),
            new_page_title: String::new(),
            cursor_negative_offset: 0,
            search: Search {
                current_search: String::new(),
                search_active: false,
            },
            show_preview: false,
            show_help: false,
            sort: Sort {
                type_state: {
                    let mut new = ListState::default();
                    new.select_first();
                    new
                },
                dir_state: SortDirection::Asc,
                sort_types_array: [SortType::CreatedOn, SortType::Title],
                saved_state: (ListState::default(), SortDirection::Asc),
            },
            space_list_pos: Bounds::default(),
            page_list_pos: Bounds::default(),
            page_updated_title: String::new(),
        }
    }

    fn load_pages(&mut self, config: &Config, space_id: &str) -> Result<()> {
        self.page_list = actions::load_page_list_for_space(&config.api, space_id)?;
        self.sort_pages(SortType::CreatedOn, SortDirection::Asc);
        Ok(())
    }

    fn sort_pages(&mut self, sort_type: SortType, sort_dir: SortDirection) {
        match sort_type {
            SortType::Title => match sort_dir {
                SortDirection::Asc => self.page_list.sort_by_key(|a| a.title.clone()),
                SortDirection::Desc => self.page_list.sort_by_key(|a| Reverse(a.title.clone())),
            },
            SortType::CreatedOn => match sort_dir {
                SortDirection::Asc => self.page_list.sort_by_key(|a| a.get_date_created()),
                SortDirection::Desc => self
                    .page_list
                    .sort_by_key(|a| Reverse(a.get_date_created())),
            },
        }
    }

    // Gets the currently selected space based on the app state
    // Combines the space list and the space list state
    fn get_selected_space(&self) -> Option<Space> {
        if let Some(selected_index) = self.space_list_state.selected() {
            return self.space_list.get(selected_index).cloned();
        }
        None
    }

    // Gets the currently selected page based on the app state
    // Combines the page list and page list state
    // This is much more likely to return None than the space function above
    // as there is often no page selected (when changing space options for instance)
    fn get_selected_page(&self) -> Option<Page> {
        if let Some(selected_index) = self.page_list_state.selected() {
            return self.page_list.get(selected_index).cloned();
        }
        None
    }

    // Helper functions that enable both lists to be manipulated without duplicate calls
    // Also handle list wrapping
    fn list_next(&mut self) {
        let (list_state, list_length) = match self.current_area {
            CurrentArea::Spaces => {
                let list_length = self.space_list.len();
                (&mut self.space_list_state, list_length)
            }
            CurrentArea::Pages => {
                let list_length = self.page_list.len();
                (&mut self.page_list_state, list_length)
            }
            CurrentArea::SortPopup => (&mut self.sort.type_state, self.sort.sort_types_array.len()),
            // List nav keys don't do anything unless we're focused on a list, so return
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

    fn list_previous(&mut self) {
        let list_state = match self.current_area {
            CurrentArea::Spaces => &mut self.space_list_state,
            CurrentArea::Pages => &mut self.page_list_state,
            CurrentArea::SortPopup => &mut self.sort.type_state,
            // List nav keys don't do anything unless we're focused on a list, so return
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

    fn refresh_current_list(&mut self, config: &Config) -> Result<()> {
        match &self.current_area {
            CurrentArea::Pages => self.load_pages(
                config,
                &self
                    .get_selected_space()
                    .expect("If we're in the pages pane there must be a selected space")
                    .id,
            ),
            CurrentArea::Spaces => {
                self.space_list = actions::load_space_list(&config.api)?;
                Ok(())
            }
            s => panic!("Refresh should not be called from {:?}", s),
        }
    }

    fn backspace_text(&mut self) {
        let current_text = match self.current_area {
            CurrentArea::NewPagePopup => &mut self.new_page_title,
            CurrentArea::SearchPopup => &mut self.search.current_search,
            CurrentArea::TitlePopup => &mut self.page_updated_title,
            _ => return,
        };
        let current_length = current_text.len();
        // Make sure the text is not empty and the cursor is not right at the start
        if (current_length != 0) && (current_length != self.cursor_negative_offset) {
            // Shouldn't be able to error because of the check above but sat sub just in case
            let current_cursor_position =
            // +1 because we remove the text "before" the cursor
                current_length.saturating_sub(self.cursor_negative_offset + 1);
            current_text.remove(current_cursor_position);
        }
    }

    fn cursor_left(&mut self) {
        let current_text = match self.current_area {
            CurrentArea::NewPagePopup => &mut self.new_page_title,
            CurrentArea::SearchPopup => &mut self.search.current_search,
            CurrentArea::TitlePopup => &mut self.page_updated_title,
            _ => return,
        };
        let current_title_length = current_text.len();
        // If we're not at the start of the text, then move left i.e. increase the negative
        // position
        if self.cursor_negative_offset < current_title_length {
            self.cursor_negative_offset += 1;
        };
    }

    fn cursor_right(&mut self) {
        // Decrease the offset, saturating at 0
        self.cursor_negative_offset = self.cursor_negative_offset.saturating_sub(1);
    }

    fn type_char(&mut self, char: char) {
        let current_text = match self.current_area {
            CurrentArea::NewPagePopup => &mut self.new_page_title,
            CurrentArea::SearchPopup => &mut self.search.current_search,
            CurrentArea::TitlePopup => &mut self.page_updated_title,
            _ => return,
        };
        let current_cursor_position = current_text.len() - self.cursor_negative_offset;
        current_text.insert(current_cursor_position, char);
    }

    // Should be called any time the text entry box is exited
    fn reset_cursor(&mut self) {
        self.cursor_negative_offset = 0;
    }

    // Get the states of the sort options and pick the corresponding sort type from the saved
    // arrays
    fn get_selected_sort(&self) -> Option<(SortType, SortDirection)> {
        if let Some(selected_type) = self.sort.type_state.selected() {
            return Some((
                self.sort.sort_types_array[selected_type],
                self.sort.dir_state,
            ));
        };
        None
    }

    // Wrapper for sort_pages that checks and saves the current list states
    fn set_sort(&mut self) {
        self.sort.saved_state = (self.sort.type_state.clone(), self.sort.dir_state);
        if let Some((selected_type, selected_dir)) = self.get_selected_sort() {
            self.sort_pages(selected_type, selected_dir);
        };
    }

    fn reset_sort_state(&mut self) {
        let (type_state, dir_state) = self.sort.saved_state.clone();
        self.sort.type_state = type_state;
        self.sort.dir_state = dir_state;
    }

    fn toggle_sort_dir(&mut self) {
        match self.sort.dir_state {
            SortDirection::Asc => self.sort.dir_state = SortDirection::Desc,
            SortDirection::Desc => self.sort.dir_state = SortDirection::Asc,
        }
    }

    fn reset_search(&mut self) {
        if self.search.search_active {
            self.search.search_active = false;
            self.search.current_search = String::new();
        };
    }

    fn reset_sort(&mut self) {
        self.sort = Sort {
            // Select the first item by default
            type_state: {
                let mut new = ListState::default();
                new.select_first();
                new
            },
            dir_state: SortDirection::Asc,
            sort_types_array: [SortType::CreatedOn, SortType::Title],
            saved_state: (ListState::default(), SortDirection::Asc),
        };
    }

    fn clear_page_saved_state(&mut self, page_id: &str) {
        self.page_states_map.remove(page_id);
    }

    fn toggle_preview(&mut self) {
        self.show_preview = !self.show_preview;
    }

    fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    fn mouse_select_list(&mut self, x: u16, y: u16) {
        let (list_state, list_pos, list_length) = match self.current_area {
            CurrentArea::Spaces => (
                &mut self.space_list_state,
                &self.space_list_pos,
                &self.space_list.len(),
            ),
            CurrentArea::Pages => (
                &mut self.page_list_state,
                &self.page_list_pos,
                &self.page_list.len(),
            ),
            // List nav keys don't do anything unless we're focused on a list, so return
            _ => return,
        };
        let top_ui_offset = list_pos.top + 1;
        if x <= list_pos.left
            || x >= list_pos.right
            || y <= list_pos.top
            || y as usize >= list_length - list_state.offset() + top_ui_offset as usize
        {
            list_state.select(None);
            return;
        }
        let mouse_list_selection_point: i16 = y as i16 - top_ui_offset as i16;
        let mouse_list_selection_index = mouse_list_selection_point + list_state.offset() as i16;
        if mouse_list_selection_index >= 0 {
            list_state.select(Some(mouse_list_selection_index as usize));
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
    Backspace,
    TypeChar(char),
    CursorLeft,
    CursorRight,
    DeletePage,
    ConfirmDeletePage,
    CancelDeletePage,
    StartSearch,
    ConfirmSearch,
    CancelSearch,
    TogglePreview,
    ToggleHelp,
    StartSort,
    ConfirmSort,
    CancelSort,
    ToggleSortDir,
    MouseSelect(u16, u16),
    UpdateTitle,
    ConfirmTitle,
    CancelTitle,
}

// Possible states for an edited page to end up in
// Note that there is an implicit third option "not edited"
#[derive(Clone, Debug)]
enum PageState {
    NotSaved,
    Saved,
}

// Represents the current list the user is selecting
#[derive(Clone, Debug)]
enum CurrentArea {
    Spaces,
    Pages,
    SavePopup,
    NewPagePopup,
    DeletePopup,
    SearchPopup,
    SortPopup,
    TitlePopup,
}

// Entry point for the TUI
pub fn display(config: &Config) -> Result<()> {
    let mut terminal = ratatui::init();
    stdout().execute(EnableMouseCapture)?;
    terminal.draw(draw_start_screen)?;
    let spaces = actions::load_space_list(&config.api)?;
    let mut app = App::new(spaces);
    // Store the result here so we can reset the terminal even if it's an error
    let result = run(config, &mut terminal, &mut app);
    stdout().execute(DisableMouseCapture)?;
    ratatui::restore();
    result
}

fn draw_start_screen(frame: &mut Frame) {
    let container_block = Block::new().title(Line::from("Concmd".bold()).centered());
    frame.render_widget(container_block, frame.area());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Percentage(40),
            Constraint::Percentage(10),
            Constraint::Percentage(50),
        ])
        .split(frame.area());
    let loading_text = Paragraph::new(Line::from("Loading spaces...".bold())).centered();
    frame.render_widget(loading_text, layout[1]);
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

// Capture key events and return their message
fn handle_events(app: &App) -> Result<Option<Message>> {
    match event::read()? {
        Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
            Ok(handle_key_event(key_event.code, &app.current_area))
        }
        Event::Mouse(mouse_event) => Ok(handle_mouse_event(mouse_event, &app.current_area)),
        _ => Ok(None),
    }
}

fn handle_mouse_event(mouse_event: MouseEvent, current_area: &CurrentArea) -> Option<Message> {
    match current_area {
        CurrentArea::Spaces | CurrentArea::Pages => match mouse_event.kind {
            MouseEventKind::ScrollUp => Some(Message::ListPrevious),
            MouseEventKind::ScrollDown => Some(Message::ListNext),
            MouseEventKind::Down(MouseButton::Left) => {
                Some(Message::MouseSelect(mouse_event.column, mouse_event.row))
            }
            _ => None,
        },
        _ => None,
    }
}

// Match a keycode to the correct message
fn handle_key_event(key_event: KeyCode, current_area: &CurrentArea) -> Option<Message> {
    // Universal events apply across all areas
    // match for more events in future instead of if let
    match key_event {
        KeyCode::Char('q') => return Some(Message::Exit),
        KeyCode::Char('?') => return Some(Message::ToggleHelp),
        _ => {}
    }

    // Events for each area
    match current_area {
        CurrentArea::Spaces => match key_event {
            KeyCode::Up => Some(Message::ListPrevious),
            KeyCode::Down => Some(Message::ListNext),
            KeyCode::Left => Some(Message::Back),
            KeyCode::Right | KeyCode::Enter => Some(Message::Select),
            KeyCode::Char('r') => Some(Message::Refresh),
            _ => None,
        },
        CurrentArea::Pages => match key_event {
            KeyCode::Up => Some(Message::ListPrevious),
            KeyCode::Down => Some(Message::ListNext),
            KeyCode::Left => Some(Message::Back),
            KeyCode::Right | KeyCode::Enter => Some(Message::Select),
            KeyCode::Char('r') => Some(Message::Refresh),
            KeyCode::Char('n') => Some(Message::NewPage),
            KeyCode::Char('d') => Some(Message::DeletePage),
            KeyCode::Char('s') | KeyCode::Char('/') => Some(Message::StartSearch),
            KeyCode::Char('p') => Some(Message::TogglePreview),
            KeyCode::Char('o') => Some(Message::StartSort),
            KeyCode::Char('t') => Some(Message::UpdateTitle),
            _ => None,
        },
        CurrentArea::SavePopup => match key_event {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(Message::ConfirmSave),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(Message::RejectSave),
            _ => None,
        },
        CurrentArea::NewPagePopup => match key_event {
            KeyCode::Enter => Some(Message::SaveNewPage),
            KeyCode::Esc => Some(Message::CancelNewPage),
            KeyCode::Backspace => Some(Message::Backspace),
            KeyCode::Left => Some(Message::CursorLeft),
            KeyCode::Right => Some(Message::CursorRight),
            KeyCode::Char(value) => Some(Message::TypeChar(value)),
            _ => None,
        },
        CurrentArea::DeletePopup => match key_event {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(Message::ConfirmDeletePage),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(Message::CancelDeletePage),
            _ => None,
        },
        CurrentArea::SearchPopup => match key_event {
            KeyCode::Enter => Some(Message::ConfirmSearch),
            KeyCode::Esc => Some(Message::CancelSearch),
            KeyCode::Backspace => Some(Message::Backspace),
            KeyCode::Left => Some(Message::CursorLeft),
            KeyCode::Right => Some(Message::CursorRight),
            KeyCode::Char(value) => Some(Message::TypeChar(value)),
            _ => None,
        },
        CurrentArea::SortPopup => match key_event {
            KeyCode::Enter => Some(Message::ConfirmSort),
            KeyCode::Esc => Some(Message::CancelSort),
            KeyCode::Up => Some(Message::ListPrevious),
            KeyCode::Down => Some(Message::ListNext),
            KeyCode::Char('d') => Some(Message::ToggleSortDir),
            _ => None,
        },
        CurrentArea::TitlePopup => match key_event {
            KeyCode::Enter => Some(Message::ConfirmTitle),
            KeyCode::Esc => Some(Message::CancelTitle),
            KeyCode::Backspace => Some(Message::Backspace),
            KeyCode::Left => Some(Message::CursorLeft),
            KeyCode::Right => Some(Message::CursorRight),
            KeyCode::Char(value) => Some(Message::TypeChar(value)),
            _ => None,
        },
    }
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
                app.clear_page_saved_state(&page.id);
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
                    app.page_states_map.insert(page.id, PageState::Saved);
                    return Ok(Some(Message::Save));
                }
                panic!("Attempted to save without a page selected");
            }
        }
        Message::RejectSave => {
            if let CurrentArea::SavePopup = app.current_area
                && let Some(page) = app.get_selected_page()
            {
                app.current_area = CurrentArea::Pages;
                // Save the page ID to the map to flag as not saved in the UI
                app.page_states_map.insert(page.id, PageState::NotSaved);
            }
        }
        Message::Save => {
            if let CurrentArea::SavePopup = app.current_area {
                // if let Some(mut page) = app.get_selected_page() {
                let mut page = app
                    .get_selected_page()
                    .expect("Should not attempt to save without a page selected");
                actions::upload_page(
                    &config.api,
                    &mut page,
                    app.edited_file_path.as_deref(),
                    actions::UploadType::Update,
                )?;
                app.current_area = CurrentArea::Pages;
                // Refresh the page list so that pages can be edited again
                return Ok(Some(Message::Refresh));
            }
        }
        Message::Back => {
            match &app.current_area {
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
            };
            // Reset search when the user leaves the current area
            app.reset_search();
        }
        Message::Refresh => {
            app.reset_search();
            app.reset_sort();
            app.refresh_current_list(config)?;
        }
        // New page updates
        Message::NewPage => {
            app.current_area = CurrentArea::NewPagePopup;
        }
        Message::CancelNewPage => {
            app.current_area = CurrentArea::Pages;
            // Reset the new page title if the user cancelled
            app.new_page_title = String::new();
            app.reset_cursor();
        }
        Message::SaveNewPage => {
            actions::create_new_page(
                config,
                &app.get_selected_space()
                    .expect("Should always be a space selected"),
                app.new_page_title.clone(),
                None,
            )?;
            app.current_area = CurrentArea::Pages;
            app.reset_cursor();
            return Ok(Some(Message::Refresh));
        }
        // Edit current text input field
        Message::Backspace => app.backspace_text(),
        Message::CursorLeft => app.cursor_left(),
        Message::CursorRight => app.cursor_right(),
        Message::TypeChar(value) => app.type_char(value),

        Message::DeletePage => {
            if app.get_selected_page().is_some() {
                app.current_area = CurrentArea::DeletePopup;
            }
        }
        Message::ConfirmDeletePage => {
            actions::delete_page(
                &config.api,
                &app.get_selected_page()
                    .expect("Shouldn't delete without selected page"),
            )?;
            app.current_area = CurrentArea::Pages;
            return Ok(Some(Message::Refresh));
        }
        Message::CancelDeletePage => app.current_area = CurrentArea::Pages,
        Message::StartSearch => app.current_area = CurrentArea::SearchPopup,
        Message::ConfirmSearch => {
            // If there was a previous search active, get the full list before applying the new
            // search
            app.current_area = CurrentArea::Pages;
            if app.search.search_active {
                app.refresh_current_list(config)?;
            }
            app.page_list.retain(|p| {
                p.get_name()
                    .to_lowercase()
                    .contains(&app.search.current_search.to_lowercase())
            });
            app.search.search_active = true;
            app.reset_cursor();
        }
        Message::CancelSearch => {
            // If there's no search then clear the current search so the box is empty next time
            // the user tries to search. If there was a previous search, don't clear it so that
            // search is still there
            if !app.search.search_active {
                app.search.current_search = String::new();
            }
            app.current_area = CurrentArea::Pages;
            app.reset_cursor();
        }
        Message::TogglePreview => app.toggle_preview(),
        Message::ToggleHelp => app.toggle_help(),
        Message::StartSort => {
            app.current_area = CurrentArea::SortPopup;
        }
        Message::ConfirmSort => {
            app.set_sort();
            app.current_area = CurrentArea::Pages;
        }
        Message::CancelSort => {
            app.reset_sort_state();
            app.current_area = CurrentArea::Pages;
        }
        Message::ToggleSortDir => app.toggle_sort_dir(),
        Message::MouseSelect(x, y) => {
            app.mouse_select_list(x, y);
        }
        Message::UpdateTitle => {
            app.current_area = CurrentArea::TitlePopup;
            app.page_updated_title = app
                .get_selected_page()
                .expect("Should be a page selected")
                .title;
        }
        Message::CancelTitle => {
            app.current_area = CurrentArea::Pages;
            app.page_updated_title = String::new();
            app.reset_cursor();
        }
        Message::ConfirmTitle => {
            let current_page = app
                .get_selected_page()
                .expect("Should always be a page selected");
            actions::update_page_title(&config.api, &current_page, app.page_updated_title.clone())?;
            app.reset_cursor();
            app.current_area = CurrentArea::Pages;
            return Ok(Some(Message::Refresh));
        }
    }
    Ok(None)
}

fn draw(frame: &mut Frame, app: &mut App) {
    let main_title = Line::from("Concmd".bold());
    // Get the relevant instructions for each area if show_help is on
    let instructions = if app.show_help {
        match &app.current_area {
            CurrentArea::Spaces => Line::from("[r]efresh spaces | [q]uit | ? to close help "),
            CurrentArea::Pages => Line::from(
                "[r]efresh pages (clear search) | [n]ew page | [d]elete page | update [t]itle | [s]earch pages | [o]rder by | toggle [p]review | [q]uit | ? to close help ",
            ),
            CurrentArea::SavePopup => Line::from("[q]uit (without saving) "),
            CurrentArea::SortPopup => Line::from("toggle [d]irection "),
            _ => Line::from("[q]uit "),
        }
    } else {
        Line::from("press ? for help")
    };
    // Borderless block to hold the main title and the area instructions
    let container_block = Block::new()
        .title(main_title.centered())
        .title_bottom(instructions.right_aligned());

    // inner_area is the main rendering space for the app
    let inner_area = container_block.inner(frame.area());
    frame.render_widget(container_block, frame.area());

    // main layout holds the two lists and the preview area
    let main_layout = Layout::default()
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
        .border_set(border::PLAIN);

    let space_list = List::new(get_name_list(&app.space_list))
        .block(block)
        .highlight_style(
            Style::default()
                .bg(ratatui::style::Color::LightYellow)
                .fg(ratatui::style::Color::Black),
        );

    let space_layout = main_layout[0];
    frame.render_stateful_widget(space_list, space_layout, &mut app.space_list_state);
    app.space_list_pos = get_rect_bounds(&space_layout);

    // Page list block
    // Show the page block if the search returns no pages
    if !app.page_list.is_empty() || app.search.search_active {
        let title = Line::from("Pages".bold());

        let block = Block::bordered()
            .title(title.centered())
            .border_set(border::PLAIN);

        let page_marked_list = map_saved_pages(&app.page_list, &app.page_states_map);
        let page_dates_list = get_created_on_list(app.page_list.clone());

        let block_area = main_layout[1].width;

        // Iterate through the page titles, and add the dates in page_date_list to each page title
        // aligned with the right of the block
        // Make it so that the name goes 3 dot mode if it's too long for the date
        let page_date_aligned_list = zip(page_marked_list, page_dates_list).map(|(p, d)| {
            const DATE_LEN_PADDED: u16 = 13;
            let page_name_len = p.chars().count();
            let space = block_area
                .saturating_sub(
                    TryFrom::try_from(page_name_len)
                        .unwrap_or_else(|_| panic!("Page name was bigger than u16: {}", p)),
                )
                .saturating_sub(DATE_LEN_PADDED);
            if space == 0 {
                const ELLIPSES_LEN: u16 = 3;
                let page_space = block_area.saturating_sub(DATE_LEN_PADDED + ELLIPSES_LEN);
                let mut truncated_page = p.clone();
                truncated_page.truncate(usize::from(page_space));
                format!("{}...{}", truncated_page, d)
            } else {
                let padding = " ".repeat(usize::from(space));
                format!("{}{}{}", p, padding, d)
            }
        });

        let page_list = List::new(page_date_aligned_list)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(ratatui::style::Color::LightYellow)
                    .fg(ratatui::style::Color::Black),
            );
        let page_layout = main_layout[1];
        frame.render_stateful_widget(page_list, page_layout, &mut app.page_list_state);
        app.page_list_pos = get_rect_bounds(&page_layout);

        let internal_layout =
            Layout::vertical([Constraint::Length(4), Constraint::Fill(1)]).split(main_layout[2]);

        if let Some(selected_page) = app.get_selected_page() {
            let details_title = Line::from("Summary".bold());
            let details_block = Block::bordered()
                .title(details_title.centered())
                .border_set(border::PLAIN);
            let summary = Paragraph::new(Text::from(format!(
                "Title: {}\nCreated On: {}",
                selected_page.title,
                selected_page.get_date_created()
            )))
            .block(details_block)
            .left_aligned();

            frame.render_widget(summary, internal_layout[0]);

            // If there's a page selected, render a short preview of the content to the right if the
            // app is set to show previews
            if app.show_preview {
                let preview_text = actions::get_page_preview(&selected_page, 3500)
                    .expect("should always be able to preview the page");

                let title = Line::from("Preview".bold());
                let block = Block::bordered()
                    .title(title.centered())
                    .border_set(border::PLAIN);
                let preview = Paragraph::new(Text::from(preview_text))
                    .wrap(Wrap { trim: false })
                    .block(block)
                    .left_aligned();

                frame.render_widget(preview, internal_layout[1]);
            }
        }
    }

    match app.current_area {
        // Save popup block
        CurrentArea::SavePopup => {
            let block = get_popup_box("Publish Page?".bold());
            let question =
                Paragraph::new(Text::raw("Do you wish to save the edited page? [Y]es/[n]o"))
                    .wrap(Wrap { trim: false })
                    .block(block)
                    .centered();
            let area = popup_area(frame.area(), 40, 6);
            frame.render_widget(Clear, area);
            frame.render_widget(question, area);
        }
        CurrentArea::NewPagePopup => {
            let block = get_popup_box("Enter title for new page".bold());
            let page_title = Paragraph::new(app.new_page_title.clone())
                .wrap(Wrap { trim: false })
                .block(block);
            let area = popup_area(frame.area(), 40, 5);
            frame.render_widget(Clear, area);
            frame.render_widget(page_title, area);
            // x and y are offset by 2 to account for padding
            frame.set_cursor_position((
                area.x + 2 + app.new_page_title.len() as u16 - app.cursor_negative_offset as u16,
                area.y + 2,
            ));
        }
        CurrentArea::DeletePopup => {
            let block = get_popup_box("Delete page?".bold());
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
        CurrentArea::SearchPopup => {
            let block = get_popup_box("Search pages".bold());
            let current_search = Paragraph::new(app.search.current_search.clone())
                .wrap(Wrap { trim: false })
                .block(block);
            let area = popup_area(frame.area(), 40, 5);
            frame.render_widget(Clear, area);
            frame.render_widget(current_search, area);
            // x and y are offset by 2 to account for padding
            frame.set_cursor_position((
                area.x + 2 + app.search.current_search.len() as u16
                    - app.cursor_negative_offset as u16,
                area.y + 2,
            ));
        }
        CurrentArea::SortPopup => {
            let popup_area = popup_area(frame.area(), 50, 10);
            let order_block = get_popup_box("Order pages".bold());
            frame.render_widget(Clear, popup_area);
            frame.render_widget(order_block, popup_area);

            let inner_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(popup_area);

            // Render sort type list
            let type_block = Block::new().padding(Padding {
                left: 1,
                right: 1,
                top: 2,
                bottom: 1,
            });
            let type_strings = app.sort.sort_types_array.map(|t| t.to_string());
            let type_list = List::new(type_strings)
                .highlight_style(
                    Style::default()
                        .bg(ratatui::style::Color::LightYellow)
                        .fg(ratatui::style::Color::Black),
                )
                .block(type_block);
            frame.render_stateful_widget(type_list, inner_layout[0], &mut app.sort.type_state);

            // Render current ordering
            let dir_block = Block::new().padding(Padding {
                left: 1,
                right: 1,
                top: 2,
                bottom: 1,
            });
            let current_dir = Paragraph::new(format!("Order: {}", app.sort.dir_state))
                .wrap(Wrap { trim: false })
                .block(dir_block);
            frame.render_widget(current_dir, inner_layout[1]);
        }
        CurrentArea::TitlePopup => {
            let block = get_popup_box("Update page title".bold());
            let page_title = Paragraph::new(app.page_updated_title.clone())
                .wrap(Wrap { trim: false })
                .block(block);
            let area = popup_area(frame.area(), 40, 5);
            frame.render_widget(Clear, area);
            frame.render_widget(page_title, area);
            // x and y are offset by 2 to account for padding
            frame.set_cursor_position((
                area.x + 2 + app.page_updated_title.len() as u16
                    - app.cursor_negative_offset as u16,
                area.y + 2,
            ));
        }
        _ => {}
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

// Get the generic popup box
fn get_popup_box<'a>(title: impl Into<Line<'a>>) -> Block<'a> {
    Block::bordered()
        .border_style(Style::new().yellow())
        .padding(Padding {
            left: 1,
            right: 1,
            top: 1,
            bottom: 1,
        })
        .title(Title::from(title))
}

fn get_rect_bounds(layout: &Rect) -> Bounds {
    Bounds {
        left: layout.x,
        right: layout.x + layout.width,
        top: layout.y,
        // bottom: layout.y + layout.height,
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
fn get_name_list<A: Attr>(item_list: &[A]) -> Vec<String> {
    item_list.iter().map(|i| i.get_name()).collect()
}

// Maps a list of pages to their names + their status by looking up their IDs in the states hash
fn map_saved_pages(item_list: &[Page], states_hash: &HashMap<String, PageState>) -> Vec<String> {
    item_list
        .iter()
        .map(|i| match states_hash.get(&i.id) {
            Some(PageState::Saved) => {
                format!("✓ {}", i.get_name())
            }
            Some(PageState::NotSaved) => {
                format!("  {}", i.get_name())
            }
            None => format!("  {}", i.get_name()),
        })
        .collect()
}

fn get_created_on_list(page_list: Vec<Page>) -> Vec<String> {
    page_list.iter().map(|p| p.get_date_created()).collect()
}

#[test]
fn check_exit_all_areas() {
    use CurrentArea::*;
    static AREAS: [CurrentArea; 7] = [
        Spaces,
        Pages,
        SavePopup,
        NewPagePopup,
        DeletePopup,
        SearchPopup,
        SortPopup,
    ];
    for area in AREAS.iter() {
        let result = handle_key_event(KeyCode::Char('q'), &area);
        assert!(matches!(result, Some(Message::Exit)));
    }
}
