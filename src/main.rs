use chrono::{DateTime, Local};
use clap::{ArgAction, Args, Parser, Subcommand};
use colored::{ColoredString, Colorize};
use serde_derive::{Deserialize, Serialize};
use std::fmt::Debug;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

const PRIV_HOST: &str = "10.100.0.1";
// const HOST: &str = "localhost";
const PRIV_PORT: u16 = 8081;

const PUB_HOST: &str = "https://api.tami.moe";

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
    /// Sudo, but request the password visually using rofi -dmenu -password; this might not be a safe idea tho
    Sudo(SudoArgs),
}

#[derive(Args)]
struct DerivArgs {
    #[command(subcommand)]
    command: DerivCommands,
}
#[derive(Args)]
struct SudoArgs {
    /// The program to run with sudo
    program: String,
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
        Commands::Sudo(sudo_args) => {
            let password = String::from_utf8_lossy(
                Command::new("rofi")
                    .args(vec!["-dmenu", "-password", "-p", "Pass: "])
                    .output()
                    .expect("Failed to spawn rofi")
                    .stdout
                    .trim_ascii_end(),
            )
            .into_owned();

            let mut child = Command::new("sudo")
                .args(vec!["-S", &sudo_args.program])
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("Failed to spawn application");
            let mut stdin = child.stdin.take().expect("Failed to open stdin");
            std::thread::spawn(move || stdin.write_all(password.as_bytes()));
            let status = child.wait();
            dbg!(status);
        }
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
    let mut stream = TcpStream::connect((PRIV_HOST, PRIV_PORT)).unwrap();
    let request = format!(
        "{} HTTP/1.1\r\n\
        Host: {}\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\r\n
        {}",
        location,
        PRIV_HOST,
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
    let derivations = serde_json::from_str(
        &reqwest::blocking::get(format!("{PUB_HOST}/derivations"))
            .unwrap()
            .text()
            .unwrap(),
    )
    .unwrap();
    let current_system = fs::read_link("/run/current-system");
    match current_system {
        Ok(x) => match x.into_os_string().into_string() {
            Ok(x) => pretty_print(derivations, x.as_str()),
            Err(x) => {
                println!(
                    "Error parsing current system's nix store hash(OsString) to String {:?}",
                    x
                );
                pretty_print(derivations, "");
            }
        },
        Err(x) => {
            println!("Error getting the current system's store hash: {:?}", x);
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
fn table_print<const N: usize>(mut table: Vec<Vec<Fonal>>) {
    let termsize::Size { rows: _, cols } = termsize::get().unwrap();
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
        .map(|(index, str)| format!("{:^width$}", str.to_string(), width = lengths[index]))
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
    let mut line_diff = 0;
    for row in table {
        let mut data_line = row
            .iter()
            .enumerate()
            .map(|(index, str)| {
                if index != row.len() - 1 {
                    let mut fooon = ColoredString::from(format!(
                        "{:^width$}",
                        str.inner(),
                        width = lengths[index]
                    ));
                    if str.fgcolor().is_some() {
                        fooon.fgcolor = str.fgcolor();
                    }
                    let end_str = fooon.to_string();
                    line_diff = end_str.len() - fooon.input.len();
                    return end_str;
                } else {
                    format!("{:<width$}", str.to_string(), width = lengths[index])
                }
            })
            .collect::<Vec<String>>()
            .join(" | ");
        data_line.truncate(term_width + line_diff);
        line_diff = 0;
        println!("| {} |", data_line);
    }
}

enum Fonal {
    String(String),
    ColoredString(ColoredString),
}
impl Fonal {
    fn len(&self) -> usize {
        match self {
            Fonal::String(s) => s.len(),
            Fonal::ColoredString(cs) => cs.input.len(),
        }
    }
    fn inner(&self) -> String {
        match self {
            Fonal::String(s) => s.clone(),
            Fonal::ColoredString(colored_string) => colored_string.input.to_owned(),
        }
    }
    fn style(&self) -> colored::Style {
        match self {
            Fonal::String(_) => colored::Style::default(),
            Fonal::ColoredString(colored_string) => colored_string.style,
        }
    }
    fn fgcolor(&self) -> Option<colored::Color> {
        match self {
            Fonal::String(_) => None,
            Fonal::ColoredString(colored_string) => colored_string.fgcolor,
        }
    }
}
impl From<String> for Fonal {
    fn from(value: String) -> Self {
        Fonal::String(value)
    }
}

impl From<ColoredString> for Fonal {
    fn from(value: ColoredString) -> Self {
        Fonal::ColoredString(value)
    }
}
impl ToString for Fonal {
    fn to_string(&self) -> String {
        match self {
            Fonal::String(s) => s.clone(),
            Fonal::ColoredString(cs) => cs.to_string(),
        }
    }
}

fn pretty_print(derivations: Vec<Deriv>, curr_sys: &str) {
    let mut table: Vec<Vec<Fonal>> = Vec::new();
    for der in derivations {
        let mut info = "";
        if Path::new(&der.storeHash).exists() {
            info = "Cached";
        }
        if der.storeHash == curr_sys {
            info = "Running";
        }
        table.push(vec![
            der.name.into(),
            der.branch.into(),
            info.to_owned().into(),
            handle_date_to_dynamic_info(der.date_added).into(),
            der.storeHash.into(),
        ]);
    }

    table.push(vec![
        Fonal::String("Name".to_owned()),
        Fonal::String("Branch".to_owned()),
        Fonal::String("".to_owned()),
        Fonal::String("Date Added".to_owned()),
        Fonal::String("Hash".to_owned()),
    ]);

    table_print::<5>(table);
}

// This function should
fn handle_date_to_dynamic_info(date: Option<DateTime<Local>>) -> ColoredString {
    match date {
        Some(old) => {
            let now = Local::now();
            let dur = now - old;
            if dur.num_seconds() < 60 {
                return format!("{} seconds", dur.num_seconds()).green();
            }
            if dur.num_minutes() < 60 {
                return format!("{} minutes", dur.num_minutes()).green();
            }
            if dur.num_hours() < 24 {
                return format!("{} hours", dur.num_hours()).bright_yellow();
            }
            if dur.num_days() < 7 {
                return format!("{} days", dur.num_days()).yellow();
            }
            if dur.num_days() < 30 {
                return format!("{} days", dur.num_days()).red();
            }
            old.naive_local().to_string().red()
        }
        None => ColoredString::from("---".to_owned()),
    }
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

    let client = reqwest::blocking::Client::new();
    let response = client
        .get(format!("{PUB_HOST}/derivations/"))
        .body(json_payload)
        .send()
        .unwrap()
        .text()
        .unwrap();

    match parse_deriv_text(&response) {
        Ok(deriv) => {
            let password = rpassword::prompt_password("[sudo] password for later: ").unwrap();
            println!("");
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
            let mut cmd = Command::new("sudo")
                .args(vec!["-S", "gurl-apply-helper", &deriv.storeHash])
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("Some error with running gurl-apply-helper.");
            let mut stdin = cmd.stdin.take().expect("Failed to open stdin");
            std::thread::spawn(move || stdin.write_all(password.as_bytes()));
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
