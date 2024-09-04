use anyhow::{Ok, Result};
use reqwest::blocking;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Api;

#[derive(Serialize, Deserialize, Debug)]
pub struct Page {
    pub id: String,
    pub title: String,
    status: String,
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
            Body::Download(page_body) => &page_body.editor.value,
        }
    }

    // TODO: fix this logic to allow self-modification of retrived body value
    pub fn set_body(&mut self, body_value: String) {
        match &mut self.body {
            Body::Upload(storage) => storage.value = body_value,
            Body::Download(page_body) => page_body.editor.value = body_value,
        }
    }

    pub fn get_page_by_id(api: &Api, id: &String) -> Result<Page> {
        let resp = send_request(api, RequestType::GET, format!(
                "https://{}/wiki/api/v2/pages/{}?body-format=editor",
                api.confluence_domain, id
            ))?
            .text()?;
        // Ok(serde_json::from_str::<Page>(&resp)?)
        let page = serde_json::from_str::<Page>(&resp)?;
        Ok(page)
    }

    pub fn update_page_by_id(&mut self, api: &Api) -> Result<()> {
        self.version.number += 1; // don't think this works like this
        let serialised_body = serde_json::to_string(&self)?;

        let _resp = send_request(api, RequestType::PUT(serialised_body), format!(
            "https://{}/wiki/api/v2/pages/{}",
            api.confluence_domain, self.id
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

fn send_request(
    api: &Api,
    method: RequestType,
    url: String,
) -> Result<blocking::Response> {
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
