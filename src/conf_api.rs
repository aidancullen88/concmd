use anyhow::{Ok, Result, anyhow, bail};
use reqwest::blocking::{self, Response};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::Api;

// Used for generic functions over pages and spaces for the UI rendering
pub trait Attr {
    fn get_name(&self) -> String;
    fn get_id(&self) -> String;
}

#[derive(Deserialize)]
struct PageResults {
    results: Vec<Page>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Page {
    pub id: String,
    pub title: String,
    status: String,
    pub version: Option<PageVersion>,
    #[serde(rename = "spaceId")]
    space_id: Option<String>,
    body: Body,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Body {
    #[serde(alias = "editor")]
    storage: Storage,
}

// #[derive(Serialize, Deserialize, Debug, Clone)]
// struct BulkBody {
//     storage: Storage,
// }
//
// #[derive(Serialize, Deserialize, Debug, Clone)]
// struct PageBody {
//     editor: Storage,
// }

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

impl Attr for Page {
    fn get_name(&self) -> String {
        self.title.clone()
    }
    fn get_id(&self) -> String {
        self.id.clone()
    }
}

impl Page {
    // Constructor used when uploading a completely new page
    pub fn new(title: String, space_id: String) -> Page {
        let body = Body {
            storage: Storage {
                // The body is replaced by the serialised body later, so add a placeholder
                // for now
                value: String::new(),
                representation: "storage".to_string(),
            },
        };
        Page {
            id: String::default(),
            title,
            status: "current".to_string(),
            version: None,
            space_id: Some(space_id),
            body,
            created_at: None,
        }
    }
    pub fn get_body(&self) -> &str {
        &self.body.storage.value
    }

    pub fn set_body(&mut self, body_value: String) {
        self.body.storage.value = body_value;
    }

    pub fn get_date_created(&self) -> String {
        if let Some(created_at) = &self.created_at {
            let (date, _) = created_at.split_at(10);
            date.to_owned()
        } else {
            "".to_owned()
        }
    }

    pub fn get_space_id(&self) -> Option<String> {
        self.space_id.clone()
    }

    pub fn get_page_by_id(api: &Api, id: &str) -> Result<Page> {
        let resp = send_request(
            api,
            RequestType::Get,
            format!(
                "https://{}/wiki/api/v2/pages/{}?body-format=storage",
                api.confluence_domain, id
            ),
        )?;
        match resp.status().as_u16() {
            200 => Ok(serde_json::from_str::<Page>(&resp.text()?)?),
            _ => {
                let page_error = error_from_resp(resp);
                if page_error.code == "NOT_FOUND" {
                    bail!("Page not found: {}", page_error.title)
                }
                bail!("Issue fetching page: {}", page_error.title)
            }
        }
    }

    pub fn get_pages_by_title(api: &Api, title: &str) -> Result<Vec<Page>> {
        let resp = send_request(
            api,
            RequestType::Get,
            format!(
                "https://{}/wiki/api/v2/pages?title={}&body-format=storage",
                api.confluence_domain, title,
            ),
        )?;
        match resp.status().as_u16() {
            200 => Ok(serde_json::from_str::<PageResults>(&resp.text()?)?.results),
            400 => bail!("Malformed request: {}", error_from_resp(resp).title),
            401 => bail!("GET_UNAUTH"),
            _ => bail!("Unknown error: {}", error_from_resp(resp).title),
        }
    }

    pub fn update(&mut self, api: &Api) -> Result<Page> {
        let current_version = self.version.as_mut().ok_or(anyhow!(
            "Page without version information cannot be updated"
        ))?;
        current_version.number += 1;
        let serialised_body = serde_json::to_string(&self)?;
        let resp = send_request(
            api,
            RequestType::Put(serialised_body),
            format!(
                "https://{}/wiki/api/v2/pages/{}",
                api.confluence_domain, &self.id
            ),
        )?;
        match resp.status().as_u16() {
            200 => Ok(serde_json::from_str(&resp.text()?)?),
            _ => bail!("Publishing failed with error: {}", resp.text()?),
        }
    }

    pub fn update_title(&self, api: &Api, new_title: String) -> Result<()> {
        #[derive(Serialize)]
        struct TitleUpdate {
            status: String,
            title: String,
        }
        let body = serde_json::to_string(&TitleUpdate {
            status: "current".to_string(),
            title: new_title,
        })?;
        let resp = send_request(
            api,
            RequestType::Put(body),
            format!(
                "https://{}/wiki/api/v2/pages/{}/title",
                api.confluence_domain, &self.id
            ),
        )?;
        match resp.status().as_u16() {
            200 => Ok(()),
            _ => bail!(
                "Title change failed with error: {}",
                error_from_resp(resp).title
            ),
        }
    }

    pub fn create(&mut self, api: &Api) -> Result<Page> {
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
                bail!(error.get_error().title)
            }
            _ => bail!("Unknown response: {}", resp.text()?),
        }
    }

    pub fn get_pages(api: &Api, space_id: &str) -> Result<Vec<Page>> {
        let resp = send_request(
            api,
            RequestType::Get,
            format!(
                "https://{}/wiki/api/v2/pages?space-id={}&body-format=storage&limit=250",
                api.confluence_domain, space_id
            ),
        )?
        .text()?;
        let results = serde_json::from_str::<PageResults>(&resp)?;
        Ok(results.results)
    }

    pub fn delete(&self, api: &Api) -> Result<()> {
        let resp = send_request(
            api,
            RequestType::Del,
            format!(
                "https://{}/wiki/api/v2/pages/{}",
                api.confluence_domain, &self.id
            ),
        )?;
        match resp.status().as_u16() {
            204 => Ok(()),
            401 => bail!("DELETE_UNAUTH"),
            404 => bail!("NOT_FOUND"),
            _ => bail!("Bad request: {}", resp.text()?),
        }
    }
}

#[derive(Deserialize, Debug)]
struct GenericErrors {
    errors: Vec<PageError>,
}

#[derive(Deserialize, Debug)]
struct PageError {
    // status: usize,
    code: String,
    title: String,
    // detail: Option<String>,
}

impl GenericErrors {
    fn get_error(self) -> PageError {
        self.errors
            .into_iter()
            .next()
            .expect("Should always be at least one error object in list")
    }
}

fn error_from_resp(resp: Response) -> PageError {
    let error = serde_json::from_str::<GenericErrors>(
        &resp
            .text()
            .expect("Error response should be convertible to text"),
    )
    .expect("Error response should be deserialisable");
    error.get_error()
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

impl Attr for Space {
    fn get_name(&self) -> String {
        self.name.clone()
    }
    fn get_id(&self) -> String {
        self.id.clone()
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

    pub fn get_spaces_by_ids(api: &Api, id_list: &[String]) -> Result<Vec<Space>> {
        let id_list_str = id_list.join(",");
        let url = match &api.label {
            Some(label) => {
                format!(
                    "https://{}/wiki/api/v2/spaces?limit=250&labels={}&ids={}",
                    api.confluence_domain, label, id_list_str
                )
            }
            None => {
                format!(
                    "https://{}/wiki/api/v2/spaces?limit=250&type=global&ids={}",
                    api.confluence_domain, id_list_str
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
        RequestType::Del => client.delete(url),
    };
    let resp = generic_client
        .basic_auth(&api.username, Some(&api.token))
        .header("Content-type", "application/json")
        .send()?;
    Ok(resp)
}

enum RequestType {
    Get,
    Put(String),
    Post(String),
    Del,
}

impl fmt::Display for RequestType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RequestType::Get => write!(f, "GET"),
            RequestType::Put(_) => write!(f, "PUT"),
            RequestType::Post(_) => write!(f, "POST"),
            RequestType::Del => write!(f, "DEL"),
        }
    }
}
