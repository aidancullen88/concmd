use std::collections::HashMap;
use std::io::stdout;
use std::iter::zip;
use std::path::PathBuf;

use crate::conf_api::{Named, Page, Space};
use crate::{Config, actions};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use ratatui::Frame;
use ratatui::crossterm::ExecutableCommand;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::symbols::border;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Clear, List, ListState, Padding, Paragraph, Wrap};

use anyhow::{Result, bail};

/* Concmd uses the ELM architecture:
* draw the UI based on the state
* parse input events into a message
* update the state based on the message
* loop
*/

// Holds the entire state of the app
struct App {
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
    // "Universal" cursor for text entry fields. Make sure to clear after using!
    pub cursor_negative_offset: usize,
    pub search: Search,
    // Toggles for keybinds to turn features on and off
    pub show_preview: bool,
    pub show_help: bool,
}

struct Search {
    current_search: String,
    search_active: bool,
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
        }
    }

    fn load_pages(&mut self, config: &Config, space_id: &str) -> Result<()> {
        self.page_list = actions::load_page_list_for_space(config, space_id)?;
        Ok(())
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
                self.space_list = actions::load_space_list(config)?;
                Ok(())
            }
            s => panic!("Refresh should not be called from {:?}", s),
        }
    }

    fn backspace_text(&mut self) {
        let current_text = match self.current_area {
            CurrentArea::NewPagePopup => &mut self.new_page_title,
            CurrentArea::SearchPopup => &mut self.search.current_search,
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
            _ => return,
        };
        let current_cursor_position = current_text.len() - self.cursor_negative_offset;
        current_text.insert(current_cursor_position, char);
    }

    // Should be called any time the text entry box is exited
    fn reset_cursor(&mut self) {
        self.cursor_negative_offset = 0;
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
        KeyCode::Char('?') => return Some(Message::ToggleHelp),
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
            KeyCode::Char('s') => Some(Message::StartSearch),
            KeyCode::Char('p') => Some(Message::TogglePreview),
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
            KeyCode::Backspace => Some(Message::Backspace),
            KeyCode::Left => Some(Message::CursorLeft),
            KeyCode::Right => Some(Message::CursorRight),
            KeyCode::Char(value) => Some(Message::TypeChar(value)),
            _ => None,
        },
        CurrentArea::DeletePopup => match key_event.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(Message::ConfirmDeletePage),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(Message::CancelDeletePage),
            _ => None,
        },
        CurrentArea::SearchPopup => match key_event.code {
            KeyCode::Enter => Some(Message::ConfirmSearch),
            KeyCode::Esc => Some(Message::CancelSearch),
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
            if let CurrentArea::SavePopup = app.current_area
                && let Some(page) = app.get_selected_page()
            {
                app.current_area = CurrentArea::Pages;
                // Save the page ID to the map to flag as not saved in the UI
                app.page_states_map.insert(
                    page.id.expect("Page from API should always have an ID"),
                    PageState::NotSaved,
                );
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
        Message::Refresh => {
            if app.search.search_active {
                app.search.search_active = false;
                app.search.current_search = String::new();
            };
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
            actions::new_page_tui(
                config,
                &app.get_selected_space()
                    .expect("Should always be a space selected"),
                app.new_page_title.clone(),
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

        Message::DeletePage => app.current_area = CurrentArea::DeletePopup,
        Message::ConfirmDeletePage => {
            if let Some(mut page) = app.get_selected_page() {
                actions::delete_page(&config.api, &mut page)?;
            };
            app.current_area = CurrentArea::Pages;
            return Ok(Some(Message::Refresh));
        }
        Message::CancelDeletePage => app.current_area = CurrentArea::Pages,
        Message::StartSearch => app.current_area = CurrentArea::SearchPopup,
        Message::ConfirmSearch => {
            // If there was a previous search active, get the full list before applying the new
            // search
            if app.search.search_active {
                app.refresh_current_list(config)?;
            }
            app.page_list.retain(|p| {
                p.get_name()
                    .to_lowercase()
                    .contains(&app.search.current_search.to_lowercase())
            });
            app.current_area = CurrentArea::Pages;
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
        Message::TogglePreview => app.show_preview = !app.show_preview,
        Message::ToggleHelp => app.show_help = !app.show_help,
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
                "[r]efresh pages (clear search) | [n]ew page | [d]elete page | [s]earch pages | toggle [p]review | [q]uit | ? to close help ",
            ),
            CurrentArea::SavePopup => Line::from("[q]uit (without saving) "),
            _ => Line::from("[q]uit "),
        }
    } else {
        Line::from("press ? for help")
    };
    // Borderless block to hold the main title and the area instructions
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
    // Show the page block if the search returns no pages
    if !app.page_list.is_empty() || app.search.search_active {
        let title = Line::from("Pages".bold());

        let block = Block::bordered()
            .title(title.centered())
            .border_set(border::THICK);

        let page_marked_list = map_saved_pages(&app.page_list, &app.page_states_map);
        let page_dates_list = get_created_on_list(app.page_list.clone());

        let block_area = layout[1].width as usize;

        // Iterate through the page titles, and add the dates in page_date_list to each page title
        let page_date_aligned_list = zip(page_marked_list, page_dates_list).map(|(p, d)| {
            let page_name_len = p.len();
            let space = block_area.saturating_sub(page_name_len).saturating_sub(5);
            format!("{}  {:>width$}", p, d, width = space)
        });

        let page_list = List::new(page_date_aligned_list)
            .block(block)
            .highlight_style(
                Style::default()
                    .bg(ratatui::style::Color::LightYellow)
                    .fg(ratatui::style::Color::Black),
            );
        frame.render_stateful_widget(page_list, layout[1], &mut app.page_list_state);

        // If there's a page selected, render a short preview of the content to the right if the
        // app is set to show previews
        if let Some(selected_page) = app.get_selected_page()
            && app.show_preview
        {
            let preview_text = actions::get_page_preview(&selected_page, 1000)
                .expect("should always be able to preview the page");
            let preview_text_lines = preview_text.lines().count();
            // Make a box the same size as the amount of lines in the preview
            let internal_layout =
                Layout::vertical([Constraint::Length(preview_text_lines as u16)]).split(layout[2]);
            let title = Line::from("Preview".bold());
            let block = Block::bordered()
                .title(title.centered())
                .border_set(border::PLAIN);
            let preview = Paragraph::new(Text::from(
                actions::get_page_preview(&selected_page, 1000)
                    .expect("Page should always be convertable"),
            ))
            .wrap(Wrap { trim: false })
            .block(block)
            .left_aligned();

            frame.render_widget(preview, internal_layout[0]);
        }
    }

    // Save popup block
    match app.current_area {
        CurrentArea::SavePopup => {
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
            let title = Line::from("Enter new page title".bold());
            let block = Block::bordered()
                .border_style(Style::new().yellow())
                .padding(Padding {
                    left: 1,
                    right: 1,
                    top: 1,
                    bottom: 1,
                })
                .title(title.centered());
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
        CurrentArea::SearchPopup => {
            let title = Line::from("Search pages".bold());
            let block = Block::bordered()
                .border_style(Style::new().yellow())
                .padding(Padding {
                    left: 1,
                    right: 1,
                    top: 1,
                    bottom: 1,
                })
                .title(title.centered());
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
                    Some(PageState::Saved) => {
                        format!("{} {}", "✓", i.get_name())
                    }
                    Some(PageState::NotSaved) => {
                        format!("{} {}", "✕", i.get_name())
                    }
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

fn get_created_on_list(page_list: Vec<Page>) -> Vec<String> {
    page_list.iter().map(|p| p.get_date_created()).collect()
}
