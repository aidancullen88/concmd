use anyhow::Result;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;

use crate::conf_api;
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
    let mut page = conf_api::get_page_by_id(&config.key, id).unwrap();
    let file_path = save_page_to_file(&config.save_location, id, page.get_body()).unwrap(); // figure out errors here
    open_editor(&file_path);
    print!("Do you wish to publish this page: y/n?  ");

    let user_input: String = text_io::read!("{}\n");
    match user_input.as_str() {
        "y" | "Y" | "yes" | "Yes" => upload_page_by_id(&config.key, page, &file_path).unwrap(),
        _ => (),
    }
}

// Worker functions

// pub fn convert_path_to_pathbuf(location: &String) -> Result<PathBuf> {
//     Ok(expanduser::expanduser(location.clone())?)
// }

fn save_page_to_file(location: &PathBuf, id: &String, body: &String) -> Result<PathBuf> {
    let mut file_path = location.clone();
    file_path.push(id);
    file_path.set_extension("html");
    let mut file = File::create(&file_path)?;
    // " are downloaded as &quot; from confluence: replace for easy reading
    // serialising to JSON when publishing handles putting them back
    let body_unescaped = str::replace(body, "&quot;", "\"");
    file.write_all(body_unescaped.as_bytes())?;
    Ok(file_path)
}

fn open_editor(path: &PathBuf) {
    let _ = Command::new("nvim")
        .arg(path)
        .spawn()
        .expect("failed to open nvim")
        .wait()
        .expect("nvim exited with non-zero status");
}

fn upload_page_by_id(key: &Key, page: conf_api::Page, file_path: &PathBuf) -> Result<()> {
    let mut file = File::open(file_path)?;
    let mut body = String::new();
    file.read_to_string(&mut body)?;
    // Process here if needed
    conf_api::update_page_by_id(key, page.id, page.title, page.version.number, body)?;
    Ok(())
}
