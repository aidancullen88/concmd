use anyhow::Result;
use regex::Regex;
use std::borrow::Cow;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use htmd::{Element, HtmlToMarkdown};
use markup5ever_rcdom::{Node, NodeData};

use crate::conf_api::Page;
use crate::Config;
use crate::Key;

// Interface

pub fn fetch_page(_space: &String, _page: &String, _filename: &PathBuf) {
    todo!()
}

pub fn publish_page(_space: &String, _page: &String, _filename: &PathBuf) {
    todo!()
}

// full workflow for page edit: pulls page, opens nvim, pushes page
pub fn edit_page_by_id(config: &Config, id: &String) {
    let mut page = Page::get_page_by_id(&config.key, id).unwrap();
    let file_path = save_page_to_file(&config.save_location, id, page.get_body()).unwrap(); // figure out errors here
    open_editor(&file_path);
    print!("Do you wish to publish this page: y/n?  ");

    let user_input: String = text_io::read!("{}\n");
    match user_input.as_str() {
        "y" | "Y" | "yes" | "Yes" => upload_page_by_id(&config.key, &mut page, &file_path).unwrap(),
        _ => (),
    }
}

// Worker functions

fn save_page_to_file(location: &PathBuf, id: &String, body: &String) -> Result<PathBuf> {
    let mut file_path = location.clone();
    file_path.push(id);
    file_path.set_extension("md");
    let mut file = File::create(&file_path)?;
    // let body_unescaped = unescape_chars(body);
    // let body_table_replaced = remove_complex_table(&body_unescaped);
    let body_table_replaced = html2md::parse_html(body);
    file.write_all(body_table_replaced.as_bytes())?;
    Ok(file_path)
}

// fn custom_tables(ele: Element) -> Option<String> {
//     match ele.node.children.clone().into_inner().iter().nth(0) {
//         Some(header_row) => generate_table_row(header_row),
//         None => None
//     }
// }
//
// fn generate_table_row(header_row: &Node) -> Option<String> {
//     todo!("do this")
// }

fn remove_complex_table(body: &str) -> Cow<str> {
    let table_regex = Regex::new(r"<table[^>]*>").expect("regex should always complile");
    table_regex.replace_all(body, "<table>")
}

fn unescape_chars(body: &str) -> String {
    body.replace("&quot;", "\"")
        .replace("&rsquo;", "'")
        .replace("&lsquo;", "'")
        .replace("&rdquo;", "\"")
        .replace("&ldquo;", "\"")
}

fn reescape_chars(body: &String) -> String {
    body.replace("\"", "&quot;")
        .replace("'", "&rsquo;")
        .replace("'", "&lsquo;")
        .replace("\"", "&rdquo;")
        .replace("\"", "&ldquo;")
}

fn open_editor(path: &PathBuf) {
    let _ = Command::new("nvim")
        .arg(path)
        .spawn()
        .expect("failed to open nvim")
        .wait()
        .expect("nvim exited with non-zero status");
}

fn upload_page_by_id(key: &Key, page: &mut Page, file_path: &PathBuf) -> Result<()> {
    let mut file = File::open(file_path)?;
    let mut unescaped_body = String::new();
    file.read_to_string(&mut unescaped_body)?;
    page.set_body(reescape_chars(&unescaped_body));
    // Process here if needed
    page.update_page_by_id(key)?;
    Ok(())
}
