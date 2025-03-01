use chrono::{DateTime, Local, TimeDelta, Utc};
use clap::{ArgAction, Args, Parser, Subcommand};
use serde_derive::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use std::{array, fs};

const HOST: &str = "10.100.0.1";
// const HOST: &str = "localhost";
const PORT: u16 = 8081;

/// Gurl ☆:.｡.o(≧▽≦)o.｡.:☆
#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(propagate_version = true)]
#[command(infer_subcommands(true))]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure gc roots derivations on server or apply them
    // #[clap(alias = "derivations")]
    Deriv(DerivArgs),
}

#[derive(Args)]
struct DerivArgs {
    #[command(subcommand)]
    command: DerivCommands,
}

#[derive(Subcommand)]
enum DerivCommands {
    /// Make a gc root on server
    Up {
        name: String,
        store_hash: String,
        #[arg(long, short, default_value = "main")]
        branch: Option<String>,
        #[clap(long, short, action = ArgAction::SetTrue)]
        force: Option<bool>,
    },
    Ls {},
    Del {
        branch: String,
        name: String,
    },
    Apply {
        #[arg(long, short, default_value = "$HOSTNAME")]
        name: Option<String>,
        #[arg(long, short, default_value = "main")]
        branch: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    // You can check for the existence of subcommands, and if found use their
    // matches just as you would the top level cmd
    match &cli.command {
        Commands::Deriv(derivargs) => match &derivargs.command {
            DerivCommands::Up {
                name,
                store_hash,
                branch,
                force,
            } => handle_deriv_upload(name, store_hash, branch.clone(), force.clone()),
            DerivCommands::Ls {} => handle_deriv_ls(),
            DerivCommands::Apply { name, branch } => {
                handle_deriv_apply(name.clone().unwrap(), branch.clone().unwrap())
            }
            DerivCommands::Del { branch, name } => handle_deriv_del(branch.clone(), name.clone()),
        },
    }
}

fn handle_deriv_del(branch: String, name: String) {
    println!(
        "Response: {}",
        make_req(
            "DELETE /derivations/",
            Some(
                serde_json::to_string(&Deriv {
                    id: None,
                    name,
                    storeHash: "".to_owned(),
                    branch,
                    force: None,
                    date_added: None
                })
                .unwrap()
                .as_str(),
            )
        )
    )
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
struct Deriv {
    id: Option<i32>,
    name: String,
    storeHash: String,
    branch: String,
    force: Option<bool>,
    date_added: Option<DateTime<Local>>,
}
fn make_req(location: &str, json: Option<&str>) -> String {
    let mut stream = TcpStream::connect((HOST, PORT)).unwrap();
    let request = format!(
        "{} HTTP/1.1\r\n\
        Host: {}\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\r\n
        {}",
        location,
        HOST,
        json.unwrap_or("{}").len(),
        json.unwrap_or("{}")
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
        .split("\r\n\r\n")
        .last()
        .unwrap_or_default()
        .to_owned()
}

fn handle_deriv_ls() {
    let derivations = serde_json::from_str(&make_req("GET /derivations", None)).unwrap();
    let current_system = fs::read_link("/run/current-system");
    match current_system {
        Ok(x) => match x.into_os_string().into_string() {
            Ok(x) => pretty_print(derivations, x.as_str()),
            Err(x) => println!(
                "Error parsing current system's nix store hash(OsString) to String {:?}",
                x
            ),
        },
        Err(x) => {
            println!(
                "There was an error getting the current system's store hash: {:?}",
                x
            );
            pretty_print(derivations, "");
        }
    }
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct UploadHashAPI<'a> {
    storeHash: &'a str,
    name: &'a str,
    branch: String,
    force: Option<bool>,
    date_added: DateTime<Local>,
}
fn handle_deriv_upload(name: &str, hash: &str, branch: Option<String>, force: Option<bool>) {
    let date_added = Local::now();

    let payload = UploadHashAPI {
        force,
        storeHash: hash,
        name,
        date_added,
        branch: match branch {
            Some(x) => x,
            None => "main".to_owned(),
        },
    };

    let json_payload =
        serde_json::to_string(&payload).expect("Failed to serialize payload to json.");

    println!(
        "Response: {}",
        make_req("POST /derivations", Some(json_payload.as_str()))
    );
}

// TODO: Add fix this term_lenght thingy...
fn table_print<const N: usize>(mut table: Vec<Vec<String>>) {
    let termsize::Size { rows, cols } = termsize::get().unwrap();
    let term_width = (cols - 4).into();
    let mut lengths: [usize; N] = [0; N];
    for row in &table {
        for i in 0..row.len() {
            lengths[i] = lengths[i].max(row[i].len());
        }
    }
    let mut head_line = table
        .pop()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, str)| format!("{:^width$}", str, width = lengths[index]))
        .collect::<Vec<String>>()
        .join(" | ");
    head_line.truncate(term_width);
    println!("| {} |", head_line);

    let mut info_line = table
        .first()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, _str)| format!("{:-^width$}", "", width = lengths[index]))
        .collect::<Vec<String>>()
        .join(" + ");
    info_line.truncate(term_width);
    println!("+ {} +", info_line);
    for row in table {
        let mut data_line = row
            .iter()
            .enumerate()
            .map(|(index, str)| {
                if index != row.len() - 1 {
                    format!("{:^width$}", str, width = lengths[index])
                } else {
                    format!("{:<width$}", str, width = lengths[index])
                }
            })
            .collect::<Vec<String>>()
            .join(" | ");
        data_line.truncate(term_width);
        println!("| {} |", data_line);
    }
}

fn pretty_print(derivations: Vec<Deriv>, curr_sys: &str) {
    let mut table: Vec<Vec<String>> = Vec::new();
    for der in derivations {
        let mut info = "";
        if Path::new(&der.storeHash).exists() {
            info = "Cached";
        }
        if der.storeHash == curr_sys {
            info = "Running";
        }
        table.push(vec![
            der.name,
            der.branch,
            info.to_owned(),
            match der.date_added {
                Some(x) => x.naive_local().to_string(),
                None => "".to_owned(),
            },
            der.storeHash,
        ]);
    }

    table.push(vec![
        "Name".to_owned(),
        "Branch".to_owned(),
        "".to_owned(),
        "Date Added".to_owned(),
        "Hash".to_owned(),
    ]);

    table_print::<5>(table);
}

fn handle_deriv_apply(name: String, branch: String) {
    let payload =
        Deriv {
            date_added: None,
            force: None,
            id: None,
            branch,
            storeHash: "".to_owned(),
            name: if name == "$HOSTNAME" {
                String::from_utf8_lossy(
            &Command::new("hostname")
                .output()
                .expect("Could not run `hostname`(unix? command), consider adding --name manually.")
                .stdout
                .trim_ascii_end()
        ).into_owned()
            } else {
                name
            },
        };
    // println!("{:?}",payload.);
    let json_payload =
        serde_json::to_string(&payload).expect("Failed to serialize payload to json.");

    let response = make_req("GET /derivations/", Some(json_payload.as_str()));
    // println!("Response: {}", &response);
    match parse_deriv_text(&response) {
        Ok(deriv) => {
            if !Path::new(&deriv.storeHash).exists() {
                let mut cmd = Command::new("nix")
                    .arg("copy")
                    .arg("--from")
                    // .arg("ssh://root@elaina.tami.moe")
                    .arg("https://nix-cache.tami.moe")
                    .arg(&deriv.storeHash)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .expect("Some error with running nix-copy-closure.");

                // let mut cmd = Command::new("nix-copy-closure")
                //     .arg("--gzip")
                //     .arg("--from")
                //     .arg("root@elaina.tami.moe")
                //     .arg(&deriv.storeHash)
                //     .stdout(Stdio::inherit())
                //     .stderr(Stdio::inherit())
                //     .spawn()
                //     .expect("Some error with running nix-copy-closure.");
                let status = cmd.wait();
                println!("\n\nnix-copy-closure exited with status {:?}", status);
            }
            let mut cmd = Command::new("gurl-apply-helper")
                .arg(&deriv.storeHash)
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("Some error with running gurl-apply-helper.");
            let status = cmd.wait();
            match status {
                Ok(x) => println!("\n\ngurl-apply-helper exited with: {:?}", x),
                Err(x) => println!("\n\nRunning gurl-apply-helper run into an error{:?}", x),
            }
        }
        Err(x) => println!("Error parsing response from server: {:?}", x),
    }
}

fn parse_deriv_text(str: &str) -> Result<Deriv, serde_json::Error> {
    serde_json::from_str(str.split("\r\n\r\n").last().unwrap_or_default())
}
