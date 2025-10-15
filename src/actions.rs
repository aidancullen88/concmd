use anyhow::{Result, anyhow, bail};
use core::panic;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use cursive::Cursive;

use crate::Editor;
use crate::conf_api::{Page, Space};
use crate::{Api, alt_tui};
use crate::{Config, Tui};

// Interface

pub fn load_space_list(config: &Config) -> Result<Vec<Space>> {
    Space::get_spaces(&config.api)
}

pub fn load_page_list_for_space(config: &Config, space_id: &str) -> Result<Vec<Page>> {
    Page::get_pages(&config.api, space_id)
}

pub fn edit_id(config: &Config, id: &String) -> Result<()> {
    // full workflow for page edit: pulls page, opens nvim, pushes page
    let mut page = Page::get_page_by_id(&config.api, id)?;
    let file_path = edit_page(config, &mut page)?;
    match config.auto_sync {
        Some(true) => {
            println!("Page uploading...");
            upload_page(&config.api, &mut page, Some(&file_path))?;
        }
        Some(false) | None => {
            print!("Publish page: y/n?: ");
            let user_input: String = text_io::read!("{}\n");
            match user_input.as_str() {
                "y" | "Y" | "yes" | "Yes" => {
                    println!("Page uploading...");
                    upload_page(&config.api, &mut page, Some(&file_path))?;
                }
                _ => bail!("USER_CANCEL"),
            }
        }
    };
    Ok(())
}

// Shortened workflow for TUI that does not handle upload
pub fn edit_page(config: &Config, page: &mut Page) -> Result<PathBuf> {
    let file_path = save_and_edit_page(config, page)?;
    // Save the edited file for use with --edit last
    update_edited_history(
        config,
        &page
            .id
            .clone()
            .ok_or_else(|| anyhow!("Edited page did not have an ID"))?,
    )?;
    Ok(file_path)
}

pub fn edit_last_page(config: &Config) -> Result<()> {
    let history_path = get_history_path_or_default(config)?;
    let history_id = get_history_id(&history_path)?;
    edit_id(config, &history_id)
}

// Entry point for both TUI options
pub fn view_pages(config: &Config) -> Result<()> {
    match &config.tui {
        // Callback heavy nature of cursive requires a lot more setup
        Some(Tui::Cursive) => {
            let mut siv = Cursive::default();
            siv.set_user_data(config.clone());
            let id = crate::tui::display(&mut siv)?;
            edit_id(config, &id)
        }
        // Default to ratatui if option not set
        Some(Tui::Ratatui) | None => alt_tui::display(config),
    }
}

pub fn upload_existing_page(
    config: &Config,
    should_edit: &bool,
    page_path: &PathBuf,
    title: String,
) -> Result<()> {
    let user_space = select_space(&config.api)?;
    println!("Page Uploading...");
    let mut uploaded_page = new_page(config, &user_space, title, Some(page_path))?;
    if *should_edit {
        save_and_edit_page(config, &mut uploaded_page)?;
    };
    update_edited_history(
        config,
        &uploaded_page
            .id
            .expect("Uploaded page should always be assigned an ID"),
    )
}

pub fn cli_new_page(config: &Config, should_edit: &bool, title: String) -> Result<()> {
    // Let the user select the space to upload to
    let user_space = select_space(&config.api)?;
    println!("Page Uploading...");
    let mut uploaded_page = new_page(config, &user_space, title, None)?;
    if *should_edit {
        save_and_edit_page(config, &mut uploaded_page)?;
    };
    if let Some(id) = uploaded_page.id {
        update_edited_history(config, &id)
    } else {
        bail!("New page was created without id")
    }
}

pub fn new_page(
    config: &Config,
    space: &Space,
    title: String,
    page_path: Option<&Path>,
) -> Result<Page> {
    let mut new_page = Page::new(title, space.id.clone());
    upload_page(&config.api, &mut new_page, page_path)
}

pub fn upload_page(api: &Api, page: &mut Page, file_path: Option<&Path>) -> Result<Page> {
    if let Some(file_path) = file_path {
        let mut file = File::open(file_path)?;
        let mut unescaped_body = String::new();
        file.read_to_string(&mut unescaped_body)?;
        // Replace the existing page body with the converted body
        page.set_body(convert_md_to_html(&unescaped_body)?);
    };
    // "Hack" to check if we are updating a page or making a new one. Should be an explict enum
    // but...
    match page.id {
        Some(_) => page.update_page_by_id(api),
        None => page.create_page(api),
    }
}

pub fn delete_page(api: &Api, page: &mut Page) -> Result<()> {
    page.delete_page(api)
}

// Get a truncated view of the page for the TUI
pub fn get_page_preview(page: &Page, preview_length: usize) -> Result<String> {
    let body = page.get_body();
    // Get the first n chars from the string and convert to md
    convert_html_to_md(&body.chars().take(preview_length).collect::<String>())
}

// Get a preview of the page for cli --last -p
pub fn get_last_page_preview(config: &Config, preview_length: usize) -> Result<String> {
    let page = get_last_page(config)?;
    get_page_preview(&page, preview_length)
}

// Get a preview of the page for cli -i -p
pub fn get_page_preview_by_id(config: &Config, id: &str, preview_length: usize) -> Result<String> {
    let page = get_page_by_id(&config.api, id)?;
    get_page_preview(&page, preview_length)
}

pub fn get_page_by_id(api: &Api, id: &str) -> Result<Page> {
    Page::get_page_by_id(api, id)
}

// Worker functions

fn save_and_edit_page(config: &Config, page: &mut Page) -> Result<PathBuf> {
    let file_path = save_page_to_file(
        &config.save_location,
        page.id
            .as_ref()
            .expect("Editing page should always have ID"),
        page.get_body(),
    )?;
    open_editor(&file_path, config.editor.as_ref())?;
    Ok(file_path)
}

fn save_page_to_file(location: &Path, id: &str, body: &str) -> Result<PathBuf> {
    let converted_body = convert_html_to_md(body)?;
    let mut file_path = location.to_path_buf();
    file_path.push(id);
    file_path.set_extension("md");
    let mut file = File::create(&file_path)?;
    file.write_all(converted_body.as_bytes())?;
    Ok(file_path)
}

fn update_edited_history(config: &Config, id: &String) -> Result<()> {
    let history_path = get_history_path_or_default(config)?;
    std::fs::write(history_path, id)?;
    Ok(())
}

fn convert_html_to_md(body: &str) -> Result<String> {
    let mut pandoc = pandoc::new();
    pandoc.set_input_format(pandoc::InputFormat::Html, vec![]);
    pandoc.set_input(pandoc::InputKind::Pipe(body.to_string()));
    pandoc.set_output_format(pandoc::OutputFormat::Markdown, vec![]);
    pandoc.set_output(pandoc::OutputKind::Pipe);
    pandoc.add_option(pandoc::PandocOption::NoWrap);
    let output = pandoc.execute()?;
    match output {
        pandoc::PandocOutput::ToBuffer(pandoc_buff) => Ok(pandoc_buff),
        _ => panic!("Pandoc returned incorrect type"),
    }
}

fn convert_md_to_html(body: &str) -> Result<String> {
    let mut pandoc = pandoc::new();
    pandoc.set_input_format(pandoc::InputFormat::MarkdownGithub, vec![]);
    pandoc.set_input(pandoc::InputKind::Pipe(body.to_string()));
    pandoc.set_output_format(pandoc::OutputFormat::Html, vec![]);
    pandoc.set_output(pandoc::OutputKind::Pipe);
    pandoc.add_option(pandoc::PandocOption::NoWrap);
    let output = pandoc.execute()?;
    match output {
        pandoc::PandocOutput::ToBuffer(pandoc_buff) => Ok(pandoc_buff),
        _ => bail!("Pandoc returned incorrect type"),
    }
}

fn open_editor(path: &PathBuf, editor: Option<&Editor>) -> Result<()> {
    match editor {
        None => Ok(edit::edit_file(path)?),
        Some(ed) => {
            let mut cmd = Command::new(&ed.editor);
            if let Some(args) = &ed.args {
                cmd.args(args);
            };
            cmd.arg(path).spawn()?.wait()?;
            Ok(())
        }
    }
}

fn get_history_path_or_default(config: &Config) -> Result<PathBuf> {
    // If the user hasn't entered a history location in the config, default to the same location as
    // the saves
    let history_path = match &config.history_location {
        Some(path) => Path::new(path).join("history.txt"),
        None => config.save_location.clone().join("history.txt"),
    };
    if std::fs::metadata(&history_path).is_err() {
        bail!("Directory for history file does not exist");
    } else {
        Ok(history_path)
    }
}

fn get_last_page(config: &Config) -> Result<Page> {
    let history_path = get_history_path_or_default(config)?;
    let history_id = get_history_id(&history_path)?;
    get_page_by_id(&config.api, &history_id)
}

fn get_history_id(history_path: &Path) -> Result<String> {
    let history_id = String::from_utf8(std::fs::read(history_path)?)?;
    Ok(history_id)
}

fn select_space(api: &Api) -> Result<Space> {
    let space_list = get_space_list(api)?;
    Ok(user_choose_space(space_list))
}

fn get_space_list(api: &Api) -> Result<Vec<Space>> {
    Space::get_spaces(api)
}

fn user_choose_space(mut space_list: Vec<Space>) -> Space {
    println!("Available Spaces:");
    for (i, space) in space_list.iter().enumerate() {
        println!(
            "{}: {}, ID: {}, Key: {}",
            i + 1,
            &space.name,
            &space.id,
            &space.key
        );
    }
    print!("Enter the number of the space to upload to: ");
    let max_selection = space_list.len() + 1;
    let selection = loop {
        let user_input: String = text_io::read!("{}\n");
        match user_input.parse::<usize>() {
            Ok(selection) if 0 < selection && selection <= max_selection => break selection,
            _ => {
                println!("Enter a number corresponding to one of the above options!");
                continue;
            }
        }
    };
    space_list.remove(selection - 1)
}
