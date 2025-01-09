use clap::{ArgAction, Args, Parser, Subcommand};
use serde_derive::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};

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
        },
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
struct Deriv {
    id: Option<i32>,
    name: String,
    storeHash: String,
    branch: String,
    force: Option<bool>,
}
fn handle_deriv_ls() {
    let mut stream = TcpStream::connect((HOST, PORT)).unwrap();
    let request = format!(
        "GET /derivations HTTP/1.1\r\n\
        Host: {}\r\n\
        Connection: close\r\n\r\n",
        HOST,
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    // dbg!(&response);
    let derivations: Vec<Deriv> =
        serde_json::from_str(response.split("\r\n\r\n").last().unwrap_or_default()).unwrap();

    pretty_print(derivations);
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct UploadHashAPI<'a> {
    storeHash: &'a str,
    name: &'a str,
    branch: String,
    force: Option<bool>,
}
fn handle_deriv_upload(name: &str, hash: &str, branch: Option<String>, force: Option<bool>) {
    println!("Debug: {:?}", force);
    let payload = UploadHashAPI {
        force,
        storeHash: hash,
        name,
        branch: match branch {
            Some(x) => x,
            None => "main".to_owned(),
        },
    };

    let json_payload =
        serde_json::to_string(&payload).expect("Failed to serialize payload to json.");

    let mut stream = TcpStream::connect((HOST, PORT)).unwrap();

    let request = format!(
        "POST /derivations HTTP/1.1\r\n\
        Host: {}\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\r\n\
        {}",
        HOST,
        json_payload.len(),
        json_payload
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

    println!("Response: {}", response);
}

fn pretty_print(derivations: Vec<Deriv>) {
    let mut row_lengths = (0, 0, 0);
    for der in &derivations {
        row_lengths = (
            row_lengths.0.max(der.name.len()),
            row_lengths.1.max(der.branch.len()),
            row_lengths.2.max(der.storeHash.len()),
        );
    }
    let (width1, width2, width3) = row_lengths;
    println!(
        "| {:^width1$} | {:^width2$} | {:^width3$} |",
        "Name", "Branch", "Hash"
    );
    println!("+-{:-^width1$}-+-{:-^width2$}-+-{:-^width3$}-+", "", "", "");
    for der in derivations {
        println!(
            "| {:^width1$} | {:^width2$} | {:<width3$} |",
            der.name, der.branch, der.storeHash
        );
    }
}

fn handle_deriv_apply(name: String, branch: String) {
    let payload =
        Deriv {
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

    let mut stream = TcpStream::connect((HOST, PORT)).unwrap();

    let request = format!(
        "GET /derivations/ HTTP/1.1\r\n\
        Host: {}\r\n\
        Content-Type: application/json\r\n\
        Content-Length: {}\r\n\
        Connection: close\r\n\r\n\
        {}",
        HOST,
        json_payload.len(),
        json_payload
    );

    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();

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
                Err(x) => println!("\n\nRunning gurl-apply-helper run into an erro{:?}", x),
            }
        }
        Err(x) => println!("Error parsing response from server: {:?}", x),
    }
}

fn parse_deriv_text(str: &str) -> Result<Deriv, serde_json::Error> {
    serde_json::from_str(str.split("\r\n\r\n").last().unwrap_or_default())
}
