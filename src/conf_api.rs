use anyhow::{Ok, Result};
use reqwest::blocking;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Api;

pub trait Name {
    fn get_name(&self) -> String;
}

#[derive(Deserialize)]
struct PageResults {
    results: Vec<Page>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Page {
    pub id: String,
    pub title: String,
    status: String,
    pub version: PageVersion,
    body: Body,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Body {
    Download(PageBody),
    Upload(Storage),
    BulkFetch(BulkBody),
}

#[derive(Serialize, Deserialize, Debug)]
struct BulkBody {
    storage: Storage,
}

#[derive(Serialize, Deserialize, Debug)]
struct PageBody {
    editor: Storage,
}

#[derive(Serialize, Deserialize, Debug)]
struct Storage {
    value: String,
    representation: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PageVersion {
    pub number: usize,
    pub message: Option<String>,
}

impl Name for Page {
    fn get_name(&self) -> String {
        self.title.clone()
    }
}

impl Page {
    // Getter and setter for body to allow for download and upload in the same struct.
    // Confluence expects slightly different structure for upload than what it gives
    // for download. This is abstracted away here to make constructing the upload json
    // a bit easier.
    pub fn get_body(&self) -> &String {
        match &self.body {
            Body::Upload(storage) => &storage.value,
            Body::Download(page_body) => &page_body.editor.value,
            Body::BulkFetch(bulk_body) => &bulk_body.storage.value,
        }
    }

    // TODO: fix this logic to allow self-modification of retrived body value
    // current implementation:
    // when body is first downloaded it is Body::Download
    // Any time body is set, it is set to Body::Upload with the new body string
    // and the correct represetation
    pub fn set_body(&mut self, body_value: String) {
        match &mut self.body {
            Body::Upload(storage) => storage.value = body_value,
            _ => {
                let new_body = Storage {
                    value: body_value,
                    representation: "storage".to_string(),
                };
                self.body = Body::Upload(new_body)
            }
        }
    }

    pub fn get_page_by_id(api: &Api, id: &String) -> Result<Page> {
        let resp = send_request(
            api,
            RequestType::GET,
            format!(
                "https://{}/wiki/api/v2/pages/{}?body-format=editor",
                api.confluence_domain, id
            ),
        )?
        .text()?;
        Ok(serde_json::from_str::<Page>(&resp)?)
    }

    pub fn update_page_by_id(&mut self, api: &Api) -> Result<blocking::Response> {
        self.version.number += 1; // don't think this works like this
        let serialised_body = serde_json::to_string(&self)?;
        let resp = send_request(
            api,
            RequestType::PUT(serialised_body),
            format!(
                "https://{}/wiki/api/v2/pages/{}",
                api.confluence_domain, self.id
            ),
        )?;
        Ok(resp)
    }

    pub fn get_pages(api: &Api, space_id: &str) -> Result<Vec<Page>> {
        let resp = send_request(
            api,
            RequestType::GET,
            format!(
                "https://{}/wiki/api/v2/pages?space-id={}&body-format=storage",
                api.confluence_domain, space_id
            ),
        )?
        .text()?;
        println!("{:#?}", resp);
        let results = serde_json::from_str::<PageResults>(&resp)?;
        Ok(results.results)
    }
}

#[derive(Deserialize)]
struct SpaceResults {
    results: Vec<Space>,
}

#[derive(Deserialize)]
pub struct Space {
    pub id: String,
    key: String,
    pub name: String,
}

impl Name for Space {
    fn get_name(&self) -> String {
        self.name.clone()
    }
}

impl Space {
    pub fn get_spaces(api: &Api) -> Result<Vec<Space>> {
        let resp = send_request(
            api,
            RequestType::GET,
            format!(
                "https://{}/wiki/api/v2/spaces?limit=250&labels=api",
                api.confluence_domain
            ),
        )?
        .text()?;
        let results = serde_json::from_str::<SpaceResults>(&resp)?;
        Ok(results.results)
    }
}

fn send_request(api: &Api, method: RequestType, url: String) -> Result<blocking::Response> {
    let client = blocking::Client::new();
    let generic_client = match method {
        RequestType::GET => client.get(url),
        RequestType::PUT(body) => client.put(url).body(body),
    };
    let resp = generic_client
        .basic_auth(&api.username, Some(&api.token))
        .header("Content-type", "application/json")
        .send()?;
    Ok(resp)
}

enum RequestType {
    GET,
    PUT(String),
}

impl fmt::Display for RequestType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RequestType::GET => write!(f, "GET"),
            RequestType::PUT(_) => write!(f, "PUT"),
        }
    }
}
