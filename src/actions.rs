use anyhow::Result;
use core::panic;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::conf_api::{Page, Space};
use crate::Api;
use crate::Config;
use crate::Editor;

// Interface

pub fn load_space_list(config: &Config) -> Result<Vec<Space>> {
    Space::get_spaces(&config.api)
}

pub fn load_page_list_for_space(config: &Config, space_id: &str) -> Result<Vec<Page>> {
    Page::get_pages(&config.api, space_id)
}

pub fn edit_page(config: &Config, id: &String) {
    // full workflow for page edit: pulls page, opens nvim, pushes page
    let mut page = Page::get_page_by_id(&config.api, id).unwrap();
    let file_path = save_page_to_file(&config.save_location, id, page.get_body()).unwrap(); // figure out errors here
    open_editor(&file_path, &config.editor);
    // Wait here for editor to close
    print!("Publish page: y/n?: ");
    let user_input: String = text_io::read!("{}\n");
    match user_input.as_str() {
        "y" | "Y" | "yes" | "Yes" => upload_page(&config.api, &mut page, &file_path).unwrap(),
        _ => (),
    };
    
    let history_path = get_history_path_or_default(config);
    std::fs::write(history_path, id).unwrap();
}

pub fn edit_last_page(config: &Config) {
    let history_path = get_history_path_or_default(config);
    
    if !std::fs::metadata(&history_path).is_ok() {
        println!("No history file found");
        return
    }

    let history_id = std::fs::read(history_path).unwrap();
    let id_string = match String::from_utf8(history_id) {
        Ok(s) => s,
        Err(e) => panic!("Invalid UTF-8 sequence: {}", e)
    };

    edit_page(&config, &id_string.trim().to_string());
}

// Worker functions

fn save_page_to_file(location: &PathBuf, id: &String, body: &String) -> Result<PathBuf> {
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

fn upload_page(api: &Api, page: &mut Page, file_path: &PathBuf) -> Result<()> {
    let mut file = File::open(file_path)?;
    let mut unescaped_body = String::new();
    file.read_to_string(&mut unescaped_body)?;
    page.set_body(convert_md_to_html(&unescaped_body)?);
    // Process here if needed
    println!("Page uploading...");
    let resp = page.update_page_by_id(api)?;
    match resp.status().as_u16() {
        200 => println!("Upload successfully complete"),
        _ => println!("Upload errored with message: {:?}", resp.text().unwrap()),
    }
    Ok(())
}

fn get_history_path_or_default(config: &Config) -> PathBuf {
    match &config.history_location {
        Some(path) => Path::new(path).join("history.txt"),
        None => config.save_location.clone().join("history.txt")
    }
}
