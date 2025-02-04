use anyhow::Result;
use core::panic;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;

use crate::conf_api::Page;
use crate::Api;
use crate::Config;

// Interface

pub fn fetch_page(_space: &String, _page: &String, _filename: &PathBuf) {
    todo!()
}

pub fn publish_page(_space: &String, _page: &String, _filename: &PathBuf) {
    todo!()
}

// full workflow for page edit: pulls page, opens nvim, pushes page
pub fn edit_page_by_id(config: &Config, id: &String) {
    let mut page = Page::get_page_by_id(&config.api, id).unwrap();
    let file_path = save_page_to_file(&config.save_location, id, page.get_body()).unwrap(); // figure out errors here
    open_editor(&file_path, &config.editor);
    print!("Do you wish to publish this page: y/n?  ");

    let user_input: String = text_io::read!("{}\n");
    match user_input.as_str() {
        "y" | "Y" | "yes" | "Yes" => upload_page_by_id(&config.api, &mut page, &file_path).unwrap(),
        _ => (),
    }
}

// Worker functions

fn save_page_to_file(location: &PathBuf, id: &String, body: &String) -> Result<PathBuf> {
    let converted_body = convert_html_to_md(body)?;

    let mut file_path = location.clone();
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

fn open_editor(path: &PathBuf, editor: &Option<String>) {
    let (command, args) = match editor {
        None => ("vim", vec!["-c", "set columns=120", "-c", "set linebreak"]),
        Some(ed) if ed == "nvim" => ("nvim", vec!["-c", "set columns=120", "-c", "set linebreak"]),
        Some(ed) => (ed.as_str(), vec![""])
    };
    
    let _ = Command::new(command)
        .args(args)
        .arg(path)
        .spawn()
        .expect("failed to open nvim")
        .wait()
        .expect("nvim exited with non-zero status");
}

fn upload_page_by_id(api: &Api, page: &mut Page, file_path: &PathBuf) -> Result<()> {
    let mut file = File::open(file_path)?;
    let mut unescaped_body = String::new();
    file.read_to_string(&mut unescaped_body)?;
    page.set_body(convert_md_to_html(&unescaped_body)?);
    // Process here if needed
    page.update_page_by_id(api)?;
    Ok(())
}
