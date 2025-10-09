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
    let file_path = save_edit_page(config, &mut page)?;
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

    update_edited_history(config, id)
}

// Shortened workflow for TUI that does not handle upload
pub fn edit_page(config: &Config, page: &mut Page) -> Result<PathBuf> {
    let file_path = save_edit_page(config, page)?;
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
    let history_path = get_history_path_or_default(config);

    if std::fs::metadata(&history_path).is_err() {
        bail!("Directory for history file does not exist");
    }

    let history_id = std::fs::read(history_path)?;
    let id_string = String::from_utf8(history_id)?;

    edit_id(config, &id_string.trim().to_string())
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
    let user_space_id = select_space(&config.api)?;
    // Make the new page struct to upload and then upload with the file at the provided path
    let mut new_page = Page::new(title, user_space_id);
    println!("Page Uploading...");
    let mut uploaded_page = upload_page(&config.api, &mut new_page, Some(page_path))?;
    if *should_edit {
        save_edit_page(config, &mut uploaded_page)?;
    };
    update_edited_history(
        config,
        &uploaded_page
            .id
            .expect("Uploaded page should always be assigned an ID"),
    )
}

pub fn create_new_page(config: &Config, should_edit: &bool, title: String) -> Result<()> {
    // Let the user select the space to upload to
    let user_space_id = select_space(&config.api)?;
    let mut new_page = Page::new(title, user_space_id);
    println!("Page Uploading...");
    let mut uploaded_page = upload_page(&config.api, &mut new_page, None)?;
    if *should_edit {
        save_edit_page(config, &mut uploaded_page)?;
    };
    if let Some(id) = uploaded_page.id {
        update_edited_history(config, &id)
    } else {
        bail!("New page was created without id")
    }
}

pub fn new_page_tui(config: &Config, space: &Space, title: String) -> Result<()> {
    let mut new_page = Page::new(title, space.id.clone());
    upload_page(&config.api, &mut new_page, None)?;
    Ok(())
}

pub fn upload_edited_page(
    config: &Config,
    page: &mut Page,
    file_path: Option<&PathBuf>,
) -> Result<()> {
    upload_page(&config.api, page, file_path)?;
    Ok(())
}

pub fn delete_page(api: &Api, page: &mut Page) -> Result<()> {
    page.delete_page(api)
}

// Get a truncated view of the
pub fn get_page_preview(page: &Page, preview_length: usize) -> Result<String> {
    let body = page.get_body();
    // Get the first 50 chars from the string and convert to md
    convert_html_to_md(&body.chars().take(preview_length).collect::<String>())
}

// Worker functions

fn save_edit_page(config: &Config, page: &mut Page) -> Result<PathBuf> {
    let file_path = save_page_to_file(
        &config.save_location,
        page.id
            .as_ref()
            .expect("Editing page should always have ID"),
        page.get_body(),
    )?;
    open_editor(&file_path, config.editor.as_ref());
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
    let history_path = get_history_path_or_default(config);
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

fn open_editor(path: &PathBuf, editor: Option<&Editor>) {
    match editor {
        None => Command::new("vim")
            .arg(path)
            .spawn()
            .expect("editor should be available to open")
            .wait()
            .expect("editor exited with non-zero status"),
        Some(ed) => {
            let mut cmd = Command::new(&ed.editor);
            if let Some(args) = &ed.args {
                cmd.args(args);
            };
            cmd.arg(path)
                .spawn()
                .expect("failed to open editor")
                .wait()
                .expect("editor exited with non-zero status")
        }
    };
}

fn upload_page(api: &Api, page: &mut Page, file_path: Option<&PathBuf>) -> Result<Page> {
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

fn get_history_path_or_default(config: &Config) -> PathBuf {
    // If the user hasn't entered a history location in the config, default to the same location as
    // the saves
    match &config.history_location {
        Some(path) => Path::new(path).join("history.txt"),
        None => config.save_location.clone().join("history.txt"),
    }
}

fn select_space(api: &Api) -> Result<String> {
    let space_list = get_space_list(api)?;
    Ok(user_choose_space(space_list).id)
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
    let selection;
    loop {
        let user_input: String = text_io::read!("{}\n");
        selection = match user_input.parse::<usize>() {
            Ok(selection) if 0 < selection && selection <= max_selection => selection,
            _ => {
                println!("Enter a number corresponding to one of the above options!");
                continue;
            }
        };
        break;
    }
    space_list.remove(selection - 1)
}
