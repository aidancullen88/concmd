use anyhow::{Result, bail};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::Editor;
use crate::conf_api::{Page, Space};
use crate::{Api, Config};

// Interface

pub enum UploadType {
    Update,
    Create,
}

pub fn load_space_list(api: &Api) -> Result<Vec<Space>> {
    Space::get_spaces(api)
}

pub fn load_page_list_for_space(api: &Api, space_id: &str) -> Result<Vec<Page>> {
    Page::get_pages(api, space_id)
}

pub fn load_page_list_select_space(api: &Api) -> Result<Vec<Page>> {
    let selected_space = select_space(api)?;
    load_page_list_for_space(api, &selected_space.id)
}

pub fn edit_id(config: &Config, id: &str) -> Result<()> {
    // full workflow for page edit: pulls page, opens nvim, pushes page
    let mut page = Page::get_page_by_id(&config.api, id)?;
    let file_path = edit_page(config, &page)?;

    match config.auto_sync {
        Some(true) => {
            println!("Page uploading...");
            upload_page(&config.api, &mut page, Some(&file_path), UploadType::Update)?;
        }
        // Ask the user if they want to sync the page or not
        Some(false) | None => {
            let user_input = get_user_input(Some("Publish page: y/n?: "))?;
            match user_input.as_str() {
                "y" | "Y" | "yes" | "Yes" => {
                    println!("Page uploading...");
                    upload_page(&config.api, &mut page, Some(&file_path), UploadType::Update)?;
                }
                _ => bail!("USER_CANCEL"),
            }
        }
    };
    Ok(())
}

// Shortened workflow for TUI that does not handle upload
pub fn edit_page(config: &Config, page: &Page) -> Result<PathBuf> {
    let file_path = save_and_edit_page(config, page)?;
    // Save the edited file for use with --edit last
    update_edited_history(config, &page.id)?;
    Ok(file_path)
}

pub fn edit_last_page(config: &Config) -> Result<()> {
    let history_id = match get_history_id(config)? {
        Some(history_id) => history_id,
        None => bail!("Attempted to edit last page with no history record"),
    };
    edit_id(config, &history_id)
}

pub fn cli_new_page(
    config: &Config,
    should_edit: &bool,
    title: String,
    page_path: Option<&Path>,
) -> Result<()> {
    // Let the user select the space to upload to
    let user_space = select_space(&config.api)?;
    println!("Page Uploading...");
    let mut uploaded_page = create_new_page(config, &user_space, title, page_path)?;
    if *should_edit {
        let file_path = save_and_edit_page(config, &uploaded_page)?;
        upload_page(
            &config.api,
            &mut uploaded_page,
            Some(&file_path),
            UploadType::Update,
        )?;
    };

    update_edited_history(config, &uploaded_page.id)
}

// Used for TUI to create a new page
pub fn create_new_page(
    config: &Config,
    space: &Space,
    title: String,
    page_path: Option<&Path>,
) -> Result<Page> {
    let mut new_page = Page::new(title, space.id.clone());
    upload_page(&config.api, &mut new_page, page_path, UploadType::Create)
}

pub fn upload_page(
    api: &Api,
    page: &mut Page,
    file_path: Option<&Path>,
    upload_type: UploadType,
) -> Result<Page> {
    if let Some(file_path) = file_path {
        let mut file = File::open(file_path)?;
        let mut unescaped_body = String::new();
        file.read_to_string(&mut unescaped_body)?;
        // Replace the existing page body with the converted body
        page.set_body(convert_md_to_html(&mut unescaped_body)?);
    };
    // "Hack" to check if we are updating a page or making a new one. Should be an explict enum
    // but...
    match upload_type {
        UploadType::Update => page.update(api),
        UploadType::Create => page.create(api),
    }
}

pub fn delete_page_by_id(api: &Api, id: &str) -> Result<()> {
    let page = get_page_by_id(api, id)?;
    delete_page(api, &page)
}

pub fn delete_page(api: &Api, page: &Page) -> Result<()> {
    page.delete(api)
}

pub fn update_page_title(api: &Api, page: &Page, new_title: String) -> Result<()> {
    page.update_title(api, new_title)
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

pub fn convert_md_string_html() -> Result<String> {
    let mut body = String::new();
    std::io::stdin().read_to_string(&mut body)?;
    convert_md_to_html(&mut body)
}

pub fn list_page_by_title(api: &Api, title: &str) -> Result<()> {
    let page_list = Page::get_pages_by_title(api, title)?;
    let space_id_list: Vec<String> = page_list.iter().filter_map(|p| p.get_space_id()).collect();
    let space_list = Space::get_spaces_by_ids(api, &space_id_list)?;
    // construct a map for fast lookup of space names
    let space_id_name_map: HashMap<&str, &str> = space_list
        .iter()
        .map(|s| (s.id.as_str(), s.name.as_str()))
        .collect();

    // print all pages with space or "None"
    for p in page_list {
        if let Some(space_id) = p.get_space_id()
            && let Some(space_name) = space_id_name_map.get(space_id.as_str())
        {
            println!("ID: {}, Title: {}, Space: {}", p.id, p.title, space_name);
        } else {
            println!("ID: {}, Title: {}, Space: None", p.id, p.title);
        }
    }
    Ok(())
}

pub fn delete_local_files(config: &Config) -> Result<()> {
    let history_id = get_history_id(config)?;
    std::fs::remove_dir_all(&config.save_location)?;
    std::fs::create_dir(&config.save_location)?;
    if let Some(history_id) = history_id {
        let history_path = get_history_path_or_default(config)?;
        std::fs::write(history_path, history_id)?;
    };
    Ok(())
}

#[cfg(target_family = "windows")]
pub fn open_page_in_browser(url: &str, browser: &str) -> Result<()> {
    let mut cmd = Command::new("start");
    cmd.args([browser, url]);
    cmd.spawn()?;
    Ok(())
}

#[cfg(target_family = "unix")]
pub fn open_page_in_browser(url: &str, browser: &str) -> Result<()> {
    let mut cmd = Command::new(browser);
    cmd.arg(url);
    cmd.spawn()?;
    Ok(())
}

// Worker functions

fn save_and_edit_page(config: &Config, page: &Page) -> Result<PathBuf> {
    let file_path = save_page_to_file(&config.save_location, &page.id, page.get_body())?;
    open_editor(&file_path, config.editor.as_ref())?;
    Ok(file_path)
}

fn save_page_to_file(location: &Path, id: &str, body: &str) -> Result<PathBuf> {
    let converted_body = convert_html_to_md(body)?;
    let mut file_path = location.to_path_buf();
    let dir_path = file_path.clone();
    file_path.push(id);
    file_path.set_extension("md");
    let mut file = match File::create(&file_path) {
        Ok(file) => file,
        // If the directory doesn't exist, try to create it
        Err(e) if matches!(e.kind(), std::io::ErrorKind::NotFound) => {
            std::fs::create_dir(dir_path)?;
            File::create(&file_path)?
        }
        Err(e) => bail!("File creation failed with error {}", e),
    };
    file.write_all(converted_body.as_bytes())?;
    Ok(file_path)
}

fn update_edited_history(config: &Config, id: &str) -> Result<()> {
    let history_path = get_history_path_or_default(config)?;
    std::fs::write(history_path, id)?;
    Ok(())
}

fn convert_html_to_md(body: &str) -> Result<String> {
    // let mut pandoc = pandoc::new();
    // pandoc.set_input_format(pandoc::InputFormat::Html, vec![]);
    // pandoc.set_input(pandoc::InputKind::Pipe(body.to_string()));
    // pandoc.set_output_format(pandoc::OutputFormat::MarkdownGithub, vec![]);
    // pandoc.set_output(pandoc::OutputKind::Pipe);
    // pandoc.add_option(pandoc::PandocOption::NoWrap);
    // let output = pandoc.execute()?;
    // match output {
    //     pandoc::PandocOutput::ToBuffer(pandoc_buff) => Ok(pandoc_buff),
    //     _ => panic!("Pandoc returned incorrect type"),
    // }
    let converter = htmd::HtmlToMarkdown::builder()
        .options(htmd::options::Options {
            ..Default::default()
        })
        .build();
    let output = converter.convert(body)?;
    Ok(output)
}

fn convert_md_to_html(body: &mut str) -> Result<String> {
    // // let removed_content = test_remove_code_block(body);
    // let mut pandoc = pandoc::new();
    // pandoc.set_input_format(pandoc::InputFormat::MarkdownGithub, vec![]);
    // pandoc.set_input(pandoc::InputKind::Pipe(body.to_string()));
    // pandoc.set_output_format(pandoc::OutputFormat::Html, vec![]);
    // pandoc.set_output(pandoc::OutputKind::Pipe);
    // pandoc.add_option(pandoc::PandocOption::NoWrap);
    // let output = pandoc.execute()?;
    // let new_body = match output {
    //     pandoc::PandocOutput::ToBuffer(pandoc_buff) => pandoc_buff,
    //     _ => bail!("Pandoc returned incorrect type"),
    // };
    // // if let Some(content) = removed_content {
    // //     test_reinsert_content(&content, &mut new_body);
    // // }
    let new_body = markdown::to_html_with_options(body, &markdown::Options::gfm())
        .map_err(|_| anyhow::anyhow!("Failed to parse markdown"))?;
    Ok(new_body)
}

// fn test_remove_code_block(body: &mut String) -> Option<String> {
//     let start_block_position = body.find("```code/rust");
//     // take a slice from the string and find the next ```
//     if let Some(start_pos) = start_block_position {
//         println!("{}", start_pos);
//         let next_string = &body[(start_pos + 12)..];
//         println!("{}", next_string);
//         let end_block_position = next_string.find("```");
//         println!("{:?}", end_block_position);
//         let end_pos = end_block_position.map_or(body.len() - 1, |pos| pos + start_pos + 12);
//         println!("{}", end_pos);
//         let content = body[(start_pos + 13)..(end_pos - 1)].to_string();
//         body.replace_range(start_pos..(end_pos + 3), "cc:code:rust");
//         return Some(content);
//     }
//     None
// }
//
// fn test_reinsert_content(content: &str, body: &mut String) {
//     let block_position = body.find("cc:code:rust");
//     if let Some(block_start) = block_position {
//         let replacement_string = format!(
//             "<ac:structured-macro ac:name=\"code\" ac:schema-version=\"1\" ac:macro-id=\"d5f2ba10-6067-4a3e-bab1-af5f3bb9b321\"><ac:parameter ac:name=\"language\">rust</ac:parameter><ac:parameter ac:name=\"breakoutMode\">wide</ac:parameter><ac:parameter ac:name=\"breakoutWidth\">760</ac:parameter><ac:plain-text-body><![CDATA[{}]]></ac:plain-text-body></ac:structured-macro>",
//             content
//         );
//         body.replace_range((block_start - 3)..(block_start + 16), &replacement_string);
//     }
// }

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
    let history_dir_path = match &config.history_location {
        Some(path) => path,
        None => &config.save_location,
    };
    let full_history_path = history_dir_path.join("history.txt");
    if !std::fs::exists(&full_history_path)? {
        std::fs::create_dir_all(history_dir_path)?;
        std::fs::File::create(&full_history_path)?;
    };
    Ok(full_history_path)
}

fn get_last_page(config: &Config) -> Result<Page> {
    let history_id = match get_history_id(config)? {
        Some(history_id) => history_id,
        None => bail!("Attemped to get history when none present"),
    };
    get_page_by_id(&config.api, &history_id)
}

fn get_history_id(config: &Config) -> Result<Option<String>> {
    let history_path = get_history_path_or_default(config)?;
    let history_string = String::from_utf8(std::fs::read(history_path)?)?;
    match history_string.as_str() {
        his_string if his_string.chars().all(|c| c.is_alphanumeric()) => Ok(Some(history_string)),
        "" => Ok(None),
        _ => bail!("Invalid history string"),
    }
}

fn select_space(api: &Api) -> Result<Space> {
    let space_list = load_space_list(api)?;
    user_choose_space(space_list)
}

fn user_choose_space(mut space_list: Vec<Space>) -> Result<Space> {
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
    let max_selection = space_list.len() + 1;
    let selection = loop {
        let user_input = get_user_input(Some("Enter the number of the space to select: "))?;
        match user_input.parse::<usize>() {
            Ok(selection) if 0 < selection && selection <= max_selection => break selection,
            _ => {
                println!("Enter a number corresponding to one of the above options!");
                continue;
            }
        }
    };
    Ok(space_list.remove(selection - 1))
}

fn get_user_input(prompt_option: Option<&str>) -> Result<String> {
    if let Some(prompt) = prompt_option {
        print!("{}", prompt);
    }
    std::io::stdout().flush()?;
    let user_input = std::io::stdin()
        .lines()
        .next()
        .expect("Should always be a user input line")?;
    Ok(user_input)
}
