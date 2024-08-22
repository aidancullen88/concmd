use anyhow::{Ok, Result};
use reqwest::blocking;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Key;

#[derive(Serialize, Deserialize, Debug)]
pub struct Page {
    pub id: String,
    pub title: String,
    status: String,
    pub version: PageVersion,
    body: Body,
}

impl Page {
    // easier to do it like this rather than have everything public
    pub fn get_body(&self) -> &String {
        match &self.body {
            Body::Upload(storage) => &storage.value,
            Body::Download(page_body) => &page_body.storage.value,
        }
    }

    pub fn set_body(&mut self, body_value: String) {
        match &mut self.body {
            Body::Upload(storage) => storage.value = body_value,
            Body::Download(page_body) => page_body.storage.value = body_value,
        }
    }

    pub fn get_page_by_id(key: &Key, id: &String) -> Result<Page> {
        let resp = send_request(key, RequestType::GET, format!(
                "https://{}/wiki/api/v2/pages/{}?body-format=editor",
                key.confluence_domain, id
            ))?
            .text()?;
        // Ok(serde_json::from_str::<Page>(&resp)?)
        let page = serde_json::from_str::<Page>(&resp)?;
        Ok(page)
    }

    pub fn update_page_by_id(&mut self, key: &Key) -> Result<()> {
        self.version.number += 1; // don't think this works like this
        let serialised_body = serde_json::to_string(&self)?;

        let _resp = send_request(key, RequestType::PUT(serialised_body), format!(
            "https://{}/wiki/api/v2/pages/{}",
            key.confluence_domain, self.id
        ))?;
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
) -> Result<blocking::Response> {
    let client = blocking::Client::new();
    let generic_client = match method {
        RequestType::GET => client.get(url),
        RequestType::PUT(body) => client.put(url).body(body),
    };
    let resp = generic_client
        .basic_auth(&key.username, Some(&key.token))
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
