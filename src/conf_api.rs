use anyhow::{anyhow, bail, Ok, Result};
use reqwest::blocking;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Api;

// Used for generic functions over pages and spaces for the UI rendering
pub trait Named {
    fn get_name(&self) -> String;
}

#[derive(Deserialize)]
struct PageResults {
    results: Vec<Page>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Page {
    pub id: Option<String>,
    pub title: String,
    status: String,
    pub version: Option<PageVersion>,
    #[serde(rename = "spaceId")]
    space_id: Option<String>,
    #[serde(rename = "parentId")]
    parent_id: Option<String>,
    body: Body,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
enum Body {
    Download(PageBody),
    Upload(Storage),
    BulkFetch(BulkBody),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct BulkBody {
    storage: Storage,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct PageBody {
    editor: Storage,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Storage {
    value: String,
    representation: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PageVersion {
    pub number: usize,
    pub message: Option<String>,
    // #[serde(rename = "createdAt")]
    // pub created_at: String,
}

impl Named for Page {
    fn get_name(&self) -> String {
        self.title.clone()
    }
}

impl Page {
    // Constructor used when uploading a completely new page
    pub fn new(title: String, space_id: String, parent_id: String) -> Page {
        let body = Body::Upload(Storage {
            // The body is replaced by the serialised body later, so add a placeholder
            // for now
            value: "PLACEHOLDER".to_string(),
            representation: "storage".to_string(),
        });
        Page {
            id: None,
            title,
            status: "current".to_string(),
            version: None,
            space_id: Some(space_id),
            parent_id: Some(parent_id),
            body,
        }
    }
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
            RequestType::Get,
            format!(
                "https://{}/wiki/api/v2/pages/{}?body-format=editor",
                api.confluence_domain, id
            ),
        )?
        .text()?;
        Ok(serde_json::from_str::<Page>(&resp)?)
    }

    pub fn update_page_by_id(&mut self, api: &Api) -> Result<Page> {
        let current_version = self.version.as_mut().ok_or(anyhow!(
            "Page without version information cannot be updated"
        ))?;
        current_version.number += 1;
        let serialised_body = serde_json::to_string(&self)?;
        let current_id = self
            .id
            .as_ref()
            .ok_or(anyhow!("Page without an ID cannot be updated"))?;
        let resp = send_request(
            api,
            RequestType::Put(serialised_body),
            format!(
                "https://{}/wiki/api/v2/pages/{}",
                api.confluence_domain, current_id
            ),
        )?;
        match resp.status().as_u16() {
            200 => Ok(serde_json::from_str(&resp.text()?)?),
            _ => bail!("Publishing failed with error: {}", resp.text()?),
        }
    }

    pub fn create_page(&mut self, api: &Api) -> Result<Page> {
        let serialised_body = serde_json::to_string(&self)?;
        let resp = send_request(
            api,
            RequestType::Post(serialised_body),
            format!("https://{}/wiki/api/v2/pages", api.confluence_domain),
        )?;
        match &resp.status().as_u16() {
            c if *c < 300 => Ok(serde_json::from_str(&resp.text()?)?),
            c if *c >= 400 => {
                let error = serde_json::from_str::<GenericErrors>(&resp.text()?)?;
                bail!(error.get_error())
            }
            _ => bail!("Unknown response: {}", resp.text()?),
        }
    }

    pub fn get_pages(api: &Api, space_id: &str) -> Result<Vec<Page>> {
        let resp = send_request(
            api,
            RequestType::Get,
            format!(
                "https://{}/wiki/api/v2/pages?space-id={}&body-format=storage",
                api.confluence_domain, space_id
            ),
        )?
        .text()?;
        let results = serde_json::from_str::<PageResults>(&resp)?;
        Ok(results.results)
    }
}

#[derive(Deserialize, Debug)]
struct GenericErrors {
    errors: Vec<PageError>,
}

#[derive(Deserialize, Debug)]
struct PageError {
    // status: usize,
    // code: String,
    title: String,
    // detail: Option<String>,
}

impl GenericErrors {
    fn get_error(self) -> String {
        self.errors
            .into_iter()
            .next()
            .expect("Should always be at least one error object in list")
            .title
    }
}

#[derive(Deserialize, Debug)]
struct SpaceResults {
    results: Vec<Space>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Space {
    pub id: String,
    pub key: String,
    pub name: String,
}

impl Named for Space {
    fn get_name(&self) -> String {
        self.name.clone()
    }
}

impl Space {
    pub fn get_spaces(api: &Api) -> Result<Vec<Space>> {
        let url = match &api.label {
            Some(label) => {
                format!(
                    "https://{}/wiki/api/v2/spaces?limit=250&labels={}",
                    api.confluence_domain, label
                )
            }
            None => {
                format!(
                    "https://{}/wiki/api/v2/spaces?limit=250&type=global",
                    api.confluence_domain
                )
            }
        };
        let resp = send_request(api, RequestType::Get, url)?.text()?;
        let results = serde_json::from_str::<SpaceResults>(&resp)?;
        Ok(results.results)
    }
}

fn send_request(api: &Api, method: RequestType, url: String) -> Result<blocking::Response> {
    let client = blocking::Client::new();
    let generic_client = match method {
        RequestType::Get => client.get(url),
        RequestType::Put(body) => client.put(url).body(body),
        RequestType::Post(body) => client.post(url).body(body),
    };
    let resp = generic_client
        .basic_auth(&api.username, Some(&api.token))
        .header("Content-type", "application/json")
        .send()?;
    Ok(resp)
}

#[derive(Deserialize, Debug)]
struct RootPageContainer {
    results: Vec<RootPage>,
}

#[derive(Deserialize, Debug)]
pub struct RootPage {
    pub id: String,
    pub title: String,
}

impl RootPage {
    pub fn get_root_pages(api: &Api, space_id: &String) -> Result<Vec<RootPage>> {
        let resp = send_request(
            api,
            RequestType::Get,
            format!(
                "https://{}/wiki/api/v2/spaces/{}/pages?depth=root",
                api.confluence_domain, space_id
            ),
        )?
        .text()?;
        let results = serde_json::from_str::<RootPageContainer>(&resp)?;
        Ok(results.results)
    }
}

enum RequestType {
    Get,
    Put(String),
    Post(String),
}

impl fmt::Display for RequestType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RequestType::Get => write!(f, "GET"),
            RequestType::Put(_) => write!(f, "PUT"),
            RequestType::Post(_) => write!(f, "POST"),
        }
    }
}
