use anyhow::Result;
use reqwest::blocking;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Key;

#[derive(Serialize, Deserialize, Debug)]
pub struct Page {
    pub id: String,
    pub title: String,
    status: &'static str,
    pub version: PageVersion,
    body: Body,
}

impl Page {
    // Getter and setter for body to allow for download and upload in the same struct.
    // Confluence expects slightly different structure for upload than what it gives
    // for download. This is abstracted away here to make constructing the upload json
    // a bit easier.
    pub fn get_body(&self) -> &String {
        match &self.body {
            Body::Upload(storage) => &storage.value,
            Body::Download(page_body) => &page_body.storage.value,
        }
    }

    // TODO: fix this logic to allow self-modification of retrived body value
    pub fn set_body(&mut self, body_value: String) {
        fn update_body(
        match self.body {
            Body::Upload(storage) => storage.value = body_value,
            Body::Download(page_body) => page_body.storage.value = body_value,
        }
    }

    pub fn get_page_by_id(key: &Key, id: &String) -> Result<Page> {
        let client = blocking::Client::new();
        let resp = client
            .get(format!(
                "https://{}/wiki/api/v2/pages/{}?body-format=editor",
                key.confluence_domain, id
            ))
            .basic_auth(&key.username, Some(&key.token))
            .send()?
            .text()?;
        Ok(serde_json::from_str::<Page>(resp.as_str())?)
    }

    pub fn update_page_by_id(self, key: &Key) -> Result<()> {
        self.version.number += 1; // don't think this works like this
        let serialised_body = serde_json::to_string(&self)?;

        let resp = send_request(key, RequestType::PUT, format!(
            "https://{}/wiki/api/v2/pages/{}",
            key.confluence_domain, self.id
        ), serialised_body)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum Body {
    Download(PageBody),
    Upload(Storage),
}

#[derive(Serialize, Deserialize, Debug)]
struct PageBody {
    storage: Storage,
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

fn send_request(
    key: &Key,
    method: RequestType,
    url: String,
    body: String,
) -> Result<blocking::Response> {
    let client = blocking::Client::new();
    let generic_client = match method {
        RequestType::GET => client.get(url),
        RequestType::PUT => client.put(url).body(body),
    };
    let resp = generic_client
        .basic_auth(&key.username, Some(&key.token))
        .header("Content-type", "application/json")
        .send()?;
    if resp.status().is_success() {
        println!("Successfully published page!")
    } else {
        println!("Page publishing failed: {:#?}", resp.text()?)
    }
    Ok(resp)
}

enum RequestType {
    GET,
    PUT,
}

impl fmt::Display for RequestType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RequestType::GET => write!(f, "GET"),
            RequestType::PUT => write!(f, "PUT"),
        }
    }
}
