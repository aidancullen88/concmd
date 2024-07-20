use anyhow::Result;
use reqwest::blocking;
use serde::{Deserialize, Serialize};

use crate::Key;

#[derive(Serialize, Deserialize, Debug)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub version: PageVersion,
    body: PageBody,
}

impl Page {
    pub fn get_body(&self) -> &String {
        return &self.body.editor.value;
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct PageBody {
    editor: Storage,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PageVersion {
    pub number: usize,
    pub message: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Storage {
    value: String,
    representation: String,
}

#[derive(Serialize, Debug)]
pub struct PageUpdate {
    pub id: String,
    pub title: String,
    pub status: &'static str,
    version: PageVersion,
    body: Storage,
}

impl PageUpdate {
    pub fn new(id: String, title: String, version: usize, body: String) -> PageUpdate {
        let storage = Storage {
            value: body,
            representation: "storage".to_string(),
        };
        let version = PageVersion {
            number: version,
            message: None,
        };
        PageUpdate {
            id,
            title,
            version,
            status: "current",
            body: storage,
        }
    }
}

pub fn get_page_by_id(key: &Key, id: &String) -> anyhow::Result<Page> {
    let client = blocking::Client::new();
    let resp = client
        .get(format!(
            "https://{}/wiki/api/v2/pages/{}?body-format=editor",
            key.confluence_domain, id
        ))
        .basic_auth(&key.username, Some(&key.token))
        .send()?
        .text()?;
    let parsed_resp: Page = serde_json::from_str(resp.as_str())?;
    Ok(parsed_resp)
}

pub fn update_page_by_id(
    key: &Key,
    id: String,
    title: String,
    version: usize,
    body: String,
) -> Result<()> {
    let upload_body = PageUpdate::new(id.clone(), title, version + 1, body);
    let serialised_body = serde_json::to_string(&upload_body)?;

    let client = blocking::Client::new();
    let resp = client
        .put(format!(
            "https://{}/wiki/api/v2/pages/{}",
            key.confluence_domain, id
        ))
        .basic_auth(&key.username, Some(&key.token))
        .header("Content-type", "application/json")
        .body(serialised_body)
        .send()?;
    if resp.status().is_success() {
        println!("Successfully published page!")
    } else {
        println!("Page publishing failed: {:#?}", resp.text()?)
    }
    Ok(())
}
