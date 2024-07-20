mod actions;
mod conf_api;

use core::panic;
use serde::Deserialize;
use std::fs::File;
use std::{io::Read, path::PathBuf};
use toml;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    action: Action,
}

#[derive(Debug, clap::Subcommand)]
enum Action {
    Fetch {
        #[arg(short, long)]
        space: String,

        #[arg(short, long)]
        page: String,

        #[arg(short, long)]
        filename: PathBuf,
    },
    Publish {
        #[arg(short, long)]
        space: String,

        #[arg(short, long)]
        page: String,

        #[arg(short, long)]
        filename: PathBuf,
    },
    Edit {
        #[arg(short, long)]
        id: String,
    },
}

#[derive(Deserialize, Debug)]
struct Config {
    save_location: PathBuf,
    key: Key,
}

#[derive(Deserialize, Debug)]
struct Key {
    confluence_domain: String,
    username: String,
    token: String,
}

fn main() {
    let mut file = match File::open("/home/aidan/.config/concmd/config.toml") {
        Ok(file) => file,
        Err(err) => {
            panic!("{}", err)
        }
    };

    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("toml file should always be readable");
    let config: Config = toml::from_str(contents.as_str()).unwrap();

    let cli = Args::parse();

    match &cli.action {
        Action::Fetch {
            space,
            page,
            filename,
        } => crate::actions::fetch_page(space, page, filename),
        Action::Publish {
            space,
            page,
            filename,
        } => crate::actions::publish_page(space, page, filename),
        Action::Edit { id } => crate::actions::edit_page_by_id(&config, id),
    }
}
