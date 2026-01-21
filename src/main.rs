mod actions;
mod alt_tui;
mod conf_api;

use anyhow::{Context, Result};
use serde::Deserialize;

#[cfg(target_family = "unix")]
use serde::{Deserializer, de::Error};

use std::fs::File;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

use clap::{ArgGroup, Parser};

use crate::conf_api::HasAttr;

// Command line interface for clap
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    // Require either the id or the --last arg
    #[clap(group(ArgGroup::new("edit_mode").required(true).args(&["id", "last"])))]
    Edit {
        /// Re-open the last page to be edited
        #[arg(long)]
        last: bool,
        /// ID of the page to edit: cannot be used with --last
        #[arg(short, long)]
        id: Option<String>,
        /// Print n bytes of the content as a preview of the page rather than editing
        #[arg(short, long)]
        preview: Option<u16>,
    },
    View,
    New {
        /// Use an existing file to create the new page
        #[arg(long, short)]
        path: Option<String>,
        /// The title of the new page
        #[arg(long, short)]
        title: String,
        /// Open the page for editing after it has been created
        #[arg(long, short)]
        edit: bool,
    },
    Delete {
        /// ID of the page to delete
        #[arg(long, short)]
        id: String,
    },
    #[clap(group(ArgGroup::new("list_mode").required(true).args(&["pages", "spaces"])))]
    List {
        #[arg(long)]
        pages: bool,
        #[arg(short, long, requires = "pages")]
        title: Option<String>,
        #[arg(long)]
        spaces: bool,
    },
    #[clap(group(ArgGroup::new("convert_mode").required(true).args(&["md", "html"])))]
    Convert {
        #[arg(long)]
        md: bool,
        #[arg(long)]
        html: bool,
    },
    // Purges local saved pages
    Purge,
}

// Config structure. Note deserialize_with for save_location, see fn
// Deserialisation for history location requires a different function to deal with the optional
// case
#[cfg(target_family = "unix")]
#[derive(Deserialize, Debug, Clone)]
struct Config {
    #[serde(deserialize_with = "from_tilde_path")]
    save_location: PathBuf,
    #[serde(default, deserialize_with = "from_tilde_path_optional")]
    history_location: Option<PathBuf>,
    auto_sync: Option<bool>,
    api: Api,
    editor: Option<Editor>,
}

#[cfg(target_family = "windows")]
#[derive(Deserialize, Debug, Clone)]
struct Config {
    save_location: PathBuf,
    history_location: Option<PathBuf>,
    auto_sync: Option<bool>,
    api: Api,
    editor: Option<Editor>,
}

impl Config {
    fn read_config<P: AsRef<Path>>(file_name: &P) -> Result<Config> {
        let mut contents = String::new();
        let mut file = File::open(file_name).context("Config file could not be found")?;
        file.read_to_string(&mut contents)
            .context("File is not readable")?;
        toml::from_str::<Config>(contents.as_str())
            .context("The config file could not be parsed: check the formatting")
    }
}

#[derive(Deserialize, Debug, Clone)]
struct Api {
    confluence_domain: String,
    username: String,
    token: String,
    label: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct Editor {
    editor: String,
    args: Option<Vec<String>>,
}

// Implements a custom deserializer for save_location that automatically
// expands the tilde to the users home directory
#[cfg(target_family = "unix")]
fn from_tilde_path<'de, D>(deserializer: D) -> Result<PathBuf, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    expanduser::expanduser(s).map_err(D::Error::custom)
}

// Same as the above deserialiser but handles the optional case for history_location
#[cfg(target_family = "unix")]
fn from_tilde_path_optional<'de, D>(deserializer: D) -> Result<Option<PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Deserialize::deserialize(deserializer)?;
    if let Some(s) = s {
        return Ok(Some(expanduser::expanduser(s).map_err(D::Error::custom)?));
    }

    Ok(None)
}

fn main() {
    let config = match get_config() {
        Ok(c) => c,
        Err(e) => {
            println!("ERROR: Error fetching config: {}", e);
            return;
        }
    };

    let cli = Args::parse();

    match cli.action {
        Action::Edit { id, last, preview } => {
            let result =
                match (last, id, preview) {
                    (true, None, Some(preview)) => {
                        actions::get_last_page_preview(&config, preview as usize)
                            .map(|s| println!("{}", s))
                    }
                    (true, None, None) => actions::edit_last_page(&config)
                        .map(|_| println!("Page edited successfully!")),
                    (false, Some(id), Some(preview)) => {
                        actions::get_page_preview_by_id(&config, &id, preview as usize)
                            .map(|s| println!("{}", s))
                    }
                    (false, Some(id), None) => actions::edit_id(&config, &id)
                        .map(|_| println!("Page edited successfully!")),
                    _ => panic!("Unsupported CLI arguments set"),
                };

            if let Err(e) = result {
                if e.to_string() == "USER_CANCEL" {
                    println!("Exited without syncing changes")
                } else {
                    print_generic_error(e)
                }
            }
        }
        Action::View => match alt_tui::display(&config) {
            Ok(_) => {}
            Err(e) if e.to_string() == "USER_APP_EXIT" => {
                println!("Exited without selecting a page")
            }
            Err(e) => print_generic_error(e),
        },
        Action::New { path, title, edit } => {
            let result = if let Some(path) = path {
                #[cfg(target_family = "unix")]
                let expanded_path = match expanduser::expanduser(path) {
                    Ok(ex_path) => ex_path,
                    Err(_) => {
                        println!("The provided path is not valid");
                        return;
                    }
                };
                #[cfg(target_family = "windows")]
                let expanded_path = PathBuf::from(path);

                actions::cli_new_page(&config, &edit, title.clone(), Some(&expanded_path))
                    .map(|_| println!("Page created successfully!"))
            } else {
                actions::cli_new_page(
                    &config,
                    &edit,
                    title.clone(),
                    path.map(PathBuf::from).as_deref(),
                )
                .map(|_| println!("Page created successfully!"))
            };

            if let Err(e) = result {
                if e.to_string() == "USER_CANCEL" {
                    println!("Exited without saving changes");
                } else if e.to_string().starts_with("A page with this title") {
                    eprintln!(
                        "ERROR: A page with title \"{}\" already exists in this space",
                        title
                    )
                } else {
                    print_generic_error(e);
                }
            }
        }
        Action::Delete { id } => match actions::delete_page_by_id(&config.api, &id) {
            Ok(()) => println!("Page deleted successfullly"),
            Err(e) if e.to_string() == "DELETE_UNAUTH" => {
                eprintln!("Token is not authorised to delete this page")
            }
            Err(e) if e.to_string() == "NOT_FOUND" => {
                eprintln!("Page with id {} was not found for deletion", id)
            }
            Err(e) => print_generic_error(e),
        },
        Action::List {
            pages,
            spaces,
            title,
        } => match (pages, spaces, title) {
            // Case list --spaces
            (false, true, None) => match actions::load_space_list(&config.api) {
                Ok(space_list) => render_name_id_list(&space_list),
                Err(e) => print_generic_error(e),
            },
            // Case list --pages
            (true, false, None) => match actions::load_page_list_select_space(&config.api) {
                Ok(page_list) => render_name_id_list(&page_list),
                Err(e) => print_generic_error(e),
            },
            (true, false, Some(title)) => match actions::list_page_by_title(&config.api, &title) {
                Ok(()) => {}
                Err(e) => print_generic_error(e),
            },
            _ => panic!("Invalid option combination from CLI"),
        },
        Action::Convert { md, html } => match (md, html) {
            (true, false) => match actions::convert_md_string_html() {
                Ok(result_string) => println!("{}", result_string),
                Err(e) => print_generic_error(e),
            },
            (false, true) => todo!("Conversion the other way not impl yet"),
            _ => panic!("Invalid option combination from clap"),
        },
        Action::Purge => match actions::delete_local_files(&config) {
            Ok(()) => {}
            Err(e) => print_generic_error(e),
        },
    }
}

// Helper function to add the home dir to the config path. Config is always expected to live in the
// ~/.config/concmd directory.
#[cfg(target_family = "unix")]
fn get_config() -> Result<Config> {
    let config_location: PathBuf = if let Ok(xdg_config_string) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from_iter([&xdg_config_string, "/concmd/config.toml"])
    } else {
        std::env::home_dir()
            .expect("User's home directory should always exist")
            .join(".config/concmd/config.toml")
    };

    Config::read_config(&config_location)
}

#[cfg(target_family = "windows")]
fn get_config() -> Result<Config> {
    let mut home_dir = std::env::home_dir().expect("home dir should always exist");
    println!("{:?}", home_dir);
    home_dir.push("AppData\\Roaming\\concmd\\config.toml");

    println!("{:?}", home_dir);

    Config::read_config(&home_dir)
}

// Types that impl HasAttr have a name and ID to display e.g. pages, spaces
fn render_name_id_list<A: HasAttr>(items: &[A]) {
    for i in items.iter() {
        println!("ID: {}, Title: {}", i.get_id(), i.get_name())
    }
}

fn print_generic_error(e: anyhow::Error) {
    eprintln!("ERROR: {}", e)
}
