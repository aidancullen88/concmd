use anyhow::{anyhow, Result};
use core::panic;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::conf_api::{Page, RootPage, Space};
use crate::Api;
use crate::Config;
use crate::Editor;

use cursive::Cursive;

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
    edit_page(config, &mut page)?;

    update_last_edited_page(config, id)
}

pub fn edit_last_page(config: &Config) -> Result<()> {
    let history_path = get_history_path_or_default(config);

    if std::fs::metadata(&history_path).is_err() {
        return Err(anyhow!("Directory for history file does not exist"));
    }

    let history_id = std::fs::read(history_path)?;
    let id_string = String::from_utf8(history_id)?;

    edit_id(config, &id_string.trim().to_string())?;
    Ok(())
}

pub fn view_pages(config: &Config) -> Result<()> {
    let mut siv = Cursive::default();
    siv.set_user_data(config.clone());
    let id = match crate::tui::display(&mut siv) {
        Ok(id) => id,
        Err(e) => return Err(e),
    };
    edit_id(config, &id)
}

pub fn create_new_page(
    config: &Config,
    should_edit: &bool,
    page_path: &PathBuf,
    title: String,
) -> Result<()> {
    // TODO: Instead of just trying to get the root page, give a list of folders or the root to
    // choose from
    let space_list = get_space_list(&config.api)?;
    let user_selection = user_choose_space(&space_list);

    // The root page of the space is always named the same as the space. Get all the root pages
    // (usually only a few) and find the one with the same name
    let root_pages = &RootPage::get_root_pages(&config.api, &user_selection.id)?;
    let root_page_id = &root_pages
        .iter()
        .find(|x| x.title == user_selection.name)
        .expect("Should always be a root page with name matching the space")
        .id;
    // Make the new page struct to upload and then upload with the file at the provided path
    let mut new_page = Page::new(title, user_selection.id.clone(), root_page_id.clone());
    let mut uploaded_page = upload_page(&config.api, &mut new_page, page_path)?;
    if *should_edit {
        edit_page(config, &mut uploaded_page)?;
    };
    if let Some(id) = uploaded_page.id {
        update_last_edited_page(config, &id)
    } else {
        Err(anyhow!("New page was created without id"))
    }
}

// Worker functions

fn edit_page(config: &Config, page: &mut Page) -> Result<Page> {
    let file_path = save_page_to_file(
        &config.save_location,
        page.id
            .as_ref()
            .expect("Editing page should always have ID"),
        page.get_body(),
    )?;
    open_editor(&file_path, &config.editor);
    // Wait here for editor to close
    match config.auto_sync {
        Some(true) => upload_page(&config.api, page, &file_path),
        _ => {
            print!("Publish page: y/n?: ");
            let user_input: String = text_io::read!("{}\n");
            match user_input.as_str() {
                "y" | "Y" | "yes" | "Yes" => upload_page(&config.api, page, &file_path),
                _ => Err(anyhow!("ERR_USER_CANCEL")),
            }
        }
    }
}

fn save_page_to_file(location: &Path, id: &String, body: &String) -> Result<PathBuf> {
    let converted_body = convert_html_to_md(body)?;

    let mut file_path = location.to_path_buf();
    file_path.push(id);
    file_path.set_extension("md");
    let mut file = File::create(&file_path)?;
    // let body_unescaped = unescape_chars(body);
    // let body_table_replaced = remove_complex_table(&body_unescaped);
    // let body_table_replaced = html2md::parse_html(body);
    file.write_all(converted_body.as_bytes())?;
    Ok(file_path)
}

fn update_last_edited_page(config: &Config, id: &String) -> Result<()> {
    let history_path = get_history_path_or_default(config);
    std::fs::write(history_path, id)?;
    Ok(())
}

fn convert_html_to_md(body: &String) -> Result<String> {
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

fn convert_md_to_html(body: &String) -> Result<String> {
    let mut pandoc = pandoc::new();
    pandoc.set_input_format(pandoc::InputFormat::Markdown, vec![]);
    pandoc.set_input(pandoc::InputKind::Pipe(body.to_string()));
    pandoc.set_output_format(pandoc::OutputFormat::Html, vec![]);
    pandoc.set_output(pandoc::OutputKind::Pipe);
    pandoc.add_option(pandoc::PandocOption::NoWrap);
    let output = pandoc.execute()?;
    match output {
        pandoc::PandocOutput::ToBuffer(pandoc_buff) => Ok(pandoc_buff),
        _ => panic!("Pandoc returned incorrect type"),
    }
}

fn open_editor(path: &PathBuf, editor: &Option<Editor>) {
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

fn upload_page(api: &Api, page: &mut Page, file_path: &PathBuf) -> Result<Page> {
    let mut file = File::open(file_path)?;
    let mut unescaped_body = String::new();
    file.read_to_string(&mut unescaped_body)?;
    // Replace the existing page body with the converted body
    page.set_body(convert_md_to_html(&unescaped_body)?);
    println!("Page uploading...");
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

fn get_space_list(api: &Api) -> Result<Vec<Space>> {
    Space::get_spaces(api)
}

fn user_choose_space(space_list: &[Space]) -> &Space {
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
    space_list
        .get(selection - 1)
        .expect("Index is bounds checked above")
}
