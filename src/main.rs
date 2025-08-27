use chrono::{DateTime, Local};
use clap::{ArgAction, Args, Parser, Subcommand};
use colored::{ColoredString, Colorize};
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Debug;
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::{fs, u16};

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
    /// Configure gc roots derivations on server or apply them to local computer
    // #[clap(alias = "derivations")]
    Deriv(DerivArgs),
    /// Sudo, but request the password visually using `rofi -dmenu -password`; this might not be a safe idea tho
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
    /// List all derivations on server(in the DB)
    Ls {},
    /// Delete the given name on a given branch.
    /// "_" means wildcard ; `gurl deriv del auto-merge _`
    Del {
        branch: String,
        #[clap(default_value = "_")]
        name: String,
    },
    /// Apply a derivation from the server
    Apply {
        #[arg(long, short, default_value = "$HOSTNAME")]
        name: Option<String>,
        #[arg(long, short, default_value = "main")]
        branch: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

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

            let mut child = match Command::new("sudo")
                .args(vec!["-S", &sudo_args.program])
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
            {
                Ok(x) => x,
                Err(_) => {
                    return visual_println(
                        "Failed to start application with sudo?\nMaybe sudo is not installed"
                            .to_owned(),
                    )
                    .unwrap()
                }
            };

            let mut stdin = child.stdin.take().expect("Failed to open stdin");
            let handle = std::thread::spawn(move || {
                stdin
                    .write_all(password.as_bytes())
                    .expect("Failed to write to sudo child process!")
            });
            handle.join().unwrap();
            let status = child.wait();
            match status {
                Ok(exit_status) => {
                    if !exit_status.success() {
                        visual_println("Bad password!\n(exit with failure!)".to_owned()).unwrap();
                    }
                }
                Err(_) => visual_println("Failed to start program!".to_owned()).unwrap(),
            }
        }
    }
}
fn visual_println(s: String) -> Option<()> {
    let mut child = match Command::new("rofi")
        .arg("-dmenu")
        .stdin(Stdio::piped())
        .spawn()
    {
        Ok(x) => x,
        Err(_) => return None,
    };
    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let handle = std::thread::spawn(move || {
        stdin
            .write_all(s.as_bytes())
            .expect("ERROR: Failed to write to child process in `visual_println`!")
    });
    match handle.join() {
        Ok(_) => Some(()),
        Err(_) => None,
    }
}

fn handle_deriv_del(branch: String, name: String) {
    fn print_res(res: HttpResponse, name: String, branch: String) -> bool {
        if res.status.success() {
            println!("{} on the branch {}: {}", name, branch, res.body.green());
        } else {
            println!(
                "ERROR: \"{}\" on the branch \"{}\": {}",
                name,
                branch,
                res.body.red()
            );
            std::process::exit(1);
        }
        return res.status.success();
    }

    //TODO: rework so no 2 for loops when there is *1* line diff... smh
    if branch != "_" && name == "_" {
        let mut successfull = false;
        for deriv in DB::get_all()
            .unwrap()
            .into_iter()
            .filter(|x| x.branch == branch)
        {
            let res = DB::delete(&deriv.name, &deriv.branch);
            successfull |= print_res(res, deriv.name, deriv.branch);
        }
        if !successfull {
            println!(
                "ERROR: {}",
                "Failed to find any names for that branch.".red()
            );
            std::process::exit(1);
        }
    } else if branch == "_" && name != "_" {
        let mut successfull = false;
        for deriv in DB::get_all()
            .unwrap()
            .into_iter()
            .filter(|x| x.name == name)
        {
            let res = DB::delete(&deriv.name, &deriv.branch);
            successfull |= print_res(res, deriv.name, deriv.branch);
        }
        if !successfull {
            println!("ERROR: {}", "Failed to find that name on any branch.".red());
            std::process::exit(1);
        }
    } else {
        let res = DB::delete(&name, &branch);
        print_res(res, name, branch);
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
    date_added: Option<DateTime<Local>>,
}
fn make_req(location: &str, json: Option<&str>) -> Result<HttpResponse, HttpStatus> {
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
    let mut strs = response.split("\r\n\r\n");
    let status = HttpStatus::parse(strs.next().unwrap()).unwrap();
    match strs.next() {
        Some(body) => Ok(HttpResponse {
            body: body.to_string(),
            status,
        }),
        None => Err(status),
    }
}

fn handle_deriv_ls() {
    let derivations = DB::get_all().unwrap();
    let current_system = fs::read_link("/run/current-system");
    match current_system {
        Ok(x) => match x.into_os_string().into_string() {
            Ok(x) => pretty_print(derivations, x.as_str()),
            Err(x) => {
                println!(
                    "ERROR: {}",
                    format!(
                        "failed parsing current system's nix store hash(OsString) to String {:?}",
                        x
                    )
                    .red()
                );
                pretty_print(derivations, "");
                std::process::exit(1);
            }
        },
        Err(x) => {
            println!(
                "ERROR: {}",
                format!("getting the current system's store hash: {:?}", x).red()
            );
            pretty_print(derivations, "");
            std::process::exit(1);
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

    match make_upload_req(json_payload.clone()) {
        Ok(str) => print_exit(str.as_str(), 0),
        Err(err) => match err {
            UploadReqError::Comment(str) => {
                println!("{}", str);
                std::process::exit(1);
            }
            UploadReqError::StoreHashNotFound => {
                println!("INFO: uploading derivation closure to elaina");

                let private_key = std::env::var("GURL_SSH_KEY");
                let out;

                if let Ok(priv_key) = private_key {
                    println!("INFO: using ssh-agent and private key");
                    out = run_in_ssh_agent(
                        priv_key,
                        Command::new("nix")
                            .args(vec!["copy", "--to", "ssh://root@elaina.tami.moe", hash])
                            .stdout(Stdio::inherit())
                            .stderr(Stdio::inherit()),
                    );
                } else {
                    let nix_copy = Command::new("nix")
                        .args(vec!["copy", "--to", "ssh://root@elaina.tami.moe", hash])
                        .stdout(Stdio::inherit())
                        .stderr(Stdio::inherit())
                        .output();
                    out = nix_copy;
                }

                let out = out.expect("Could not run `nix` as `nix copy ...`");
                if !out.status.success() {
                    print_exit("ERROR: `nix copy ...` failed", 1);
                }
                match make_upload_req(json_payload) {
                    Ok(str) => {
                        print_exit(str.as_str(), 0);
                    }
                    Err(err) => {
                        match err {
                            UploadReqError::Comment(str) => print_exit(str.as_str(), 1),
                            UploadReqError::StoreHashNotFound => {
                                print_exit("ERROR: failed to find derivation on server even after upload...", 1);
                            }
                        };
                    }
                };
            }
        },
    };
}

fn run_in_ssh_agent(
    ssh_key: String,
    cmd: &mut Command,
) -> Result<std::process::Output, std::io::Error> {
    let mut envs = HashMap::new();

    let agent = Command::new("ssh-agent")
        .stdout(Stdio::piped())
        .output()
        .expect("failed to run `ssh-agent`");
    for line in String::from_utf8_lossy(&agent.stdout).lines() {
        if let Some(rest) = line.strip_prefix("SSH_AUTH_SOCK=") {
            envs.insert(
                "SSH_AUTH_SOCK".to_string(),
                rest.split(';').next().unwrap().to_string(),
            );
        }

        if let Some(rest) = line.strip_prefix("SSH_AGENT_PID=") {
            envs.insert(
                "SSH_AGENT_PID".to_string(),
                rest.split(';').next().unwrap().to_string(),
            );
        }
    }

    let output = inner_ssh_agent(ssh_key, cmd, &envs);

    Command::new("ssh-agent").arg("-k").envs(envs).output()?;

    output
}
fn inner_ssh_agent(
    ssh_key: String,
    cmd: &mut Command,
    envs: &HashMap<String, String>,
) -> Result<std::process::Output, std::io::Error> {
    let mut ssh_add = Command::new("ssh-add")
        .arg("-")
        .envs(envs.clone())
        .stdin(Stdio::piped())
        .spawn()?;

    let ssh_add_input = match ssh_add.stdin.as_mut() {
        Some(x) => x,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "Could not get mutable stdin of ssh-add. - Tami",
            ))
        }
    };

    ssh_add_input.write_all(ssh_key.as_bytes())?;
    ssh_add_input.write_all(b"\n")?;
    drop(ssh_add.stdin.take());

    let output = cmd.envs(envs.clone()).output();

    output
}

fn print_exit(str: &str, code: i32) {
    println!("{}", str);
    std::process::exit(code);
}
enum UploadReqError {
    Comment(String),
    StoreHashNotFound,
}

fn make_upload_req(json_payload: String) -> Result<String, UploadReqError> {
    match make_req("POST /derivations", Some(json_payload.as_str())) {
        Ok(res) => {
            if res.status.success() {
                Ok(format!(
                    "{} (server response: {})",
                    "Derivation uploaded succesffuly!".green(),
                    res.body
                ))
            } else {
                if res.status.status_code == 404 {
                    Err(UploadReqError::StoreHashNotFound)
                } else {
                    Err(UploadReqError::Comment(format!(
                        "{}\n{}",
                        format!(
                            "ERROR: {}; {} {}",
                            "failed to upload derivation".red(),
                            res.status.status_code,
                            res.status.status_message,
                        ),
                        format!("\t{}", res.body)
                    )))
                }
            }
        }
        Err(err) => Err(UploadReqError::Comment(format!(
            "ERROR: {}",
            format!("Failed to parse response body! err: {}", err.status_message).red()
        ))),
    }
}

// TODO: Add fix this term_lenght thingy...
fn table_print<const N: usize>(mut table: Vec<Vec<Fonal>>) {
    let termsize::Size { rows: _, cols } = termsize::get()
        .or(Some(termsize::Size {
            rows: 0,
            cols: u16::MAX,
        }))
        .unwrap();
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
            old.naive_local().format("%Y-%m-%d %H:%M").to_string().red()
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
    println!("INFO: name set as: {}", payload.name);
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

    let deriv: Deriv = serde_json::from_str(&response).unwrap();

    println!("INFO: this will be installed:");
    println!("\tname: {}", deriv.name);
    println!("\tbranch: {}", deriv.branch);
    println!("\thash: {}", deriv.storeHash);
    println!("\tdate: {}", handle_date_to_dynamic_info(deriv.date_added));

    let password = rpassword::prompt_password("[sudo] password for later: ").unwrap();
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
        match status {
            Ok(exit_status) => {
                if exit_status.success() {
                    println!("INFO: Closure has finished copying")
                } else {
                    println!("ERROR: {}", "Error during closure copy!".red());
                    return;
                }
            }
            Err(_) => {
                println!("ERROR: {}", "Failed to start closure copy!".red());
                return;
            }
        };
    }
    let mut cmd = Command::new("sudo")
        .args(vec!["-S", "gurl-apply-helper", &deriv.storeHash])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Some error with starting gurl-apply-helper.");
    let mut stdin = cmd.stdin.take().expect("Failed to open stdin");
    std::thread::spawn(move || stdin.write_all(password.as_bytes()));
    let status = cmd.wait();
    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!("INFO: {}", "Successfully instaleld the closure!".green());
            } else {
                println!("ERROR: {}", "Failed during closure install!".red())
            }
        }
        Err(x) => println!("ERROR: {}", "Failed to start gurl-apply-helper!".red()),
    }
}

struct DB {}
impl DB {
    pub fn get_all() -> Option<Vec<Deriv>> {
        serde_json::from_str(
            &reqwest::blocking::get(format!("{PUB_HOST}/derivations"))
                .unwrap()
                .text()
                .unwrap(),
        )
        .ok()
    }

    pub fn delete(name: &String, branch: &String) -> HttpResponse {
        make_req(
            "DELETE /derivations/",
            Some(
                serde_json::to_string(&Deriv {
                    id: None,
                    name: name.clone(),
                    storeHash: "".to_owned(),
                    branch: branch.clone(),
                    force: None,
                    date_added: None,
                })
                .unwrap()
                .as_str(),
            ),
        )
        .unwrap()
    }
}

struct HttpResponse {
    body: String,
    status: HttpStatus,
}
#[derive(Debug)]
struct HttpStatus {
    status_code: i32,
    status_message: String,
}
impl HttpStatus {
    fn parse(str: &str) -> Option<HttpStatus> {
        let str = match str.lines().next() {
            Some(x) => x,
            None => str,
        };
        let mut strs = str.split(' ');
        let _http_version = strs.next()?;
        let status_code = strs.next()?.parse().ok()?;
        let status_message = strs
            .map(|str| {
                let mut str = str.to_owned();
                str.push_str(" ");
                return str;
            })
            .collect::<String>()
            .trim()
            .to_owned();
        Some(HttpStatus {
            status_code,
            status_message,
        })
    }
    fn success(&self) -> bool {
        self.status_code == 200
    }
}
