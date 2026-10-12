use chrono::{DateTime, Local};
use clap::{ArgAction, Args, Parser, Subcommand};
use colored::{ColoredString, Colorize};
use std::fmt::Debug;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::u16;

mod deriv;
mod ssh_agent;

const PRIV_HOST: &str = "100.64.0.2";
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
    /// Print the bash eval line to so you can use the `gurl auth` command
    Init,
    /// Log into github and set up a token for `nix flake update`
    Auth,
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
    /// Rollback the current nixos profile
    Rollback {},
    /// Reapply the current system(run switch-to-configuration)
    Reapply {},
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
            } => deriv::handle_deriv_upload(name, store_hash, branch.clone(), force.clone()),
            DerivCommands::Ls {} => deriv::handle_deriv_ls(),
            DerivCommands::Apply { name, branch } => {
                deriv::handle_deriv_apply(name.clone().unwrap(), branch.clone().unwrap())
            }
            DerivCommands::Del { branch, name } => {
                deriv::handle_deriv_del(branch.clone(), name.clone())
            }
            DerivCommands::Rollback {} => deriv::handle_deriv_rollback(),
            DerivCommands::Reapply {} => deriv::handle_deriv_reapply(),
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
        Commands::Init => {
            println!("{}", include_str!("bash_eval.sh"));
        }
        Commands::Auth => {
            println!("Eval is not set up!");
            println!();
            println!("    bash copy paste:");
            println!(
                "{}",
                r#"
if command -v gurl &> /dev/null
then
    eval "$(gurl init)"
fi"#
            );
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

fn make_req(location: &str, json: Option<&str>) -> Result<DBResponse, HttpStatus> {
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
        Some(body) => Ok(DBResponse {
            message: body.to_string(),
            status,
        }),
        None => Err(status),
    }
}

fn print_exit(str: &str, code: i32) {
    println!("{}", str);
    std::process::exit(code);
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

fn pretty_print(derivations: Vec<deriv::Deriv>, curr_sys: &str) {
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
            der.repo.into(),
            der.name.into(),
            der.branch.into(),
            info.to_owned().into(),
            handle_date_to_dynamic_info(der.date_added).into(),
            der.storeHash.into(),
        ]);
    }

    table.push(vec![
        Fonal::String("Repo".to_owned()),
        Fonal::String("Name".to_owned()),
        Fonal::String("Branch".to_owned()),
        Fonal::String("".to_owned()),
        Fonal::String("Date Added".to_owned()),
        Fonal::String("Hash".to_owned()),
    ]);

    table_print::<6>(table);
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

struct DB {}
impl DB {
    pub fn get_all() -> Option<Vec<deriv::Deriv>> {
        let output = Command::new("niks3list")
            .stdout(Stdio::piped())
            .output()
            .unwrap();
        let output_string = String::from_utf8(output.stdout)
            .expect("Failed to parse `niks3 pins list` output to rust String(utf-8)");
        let mut outputs: Vec<deriv::Deriv> = Vec::new();
        for line in output_string.lines().skip(2) {
            let mut niks3_list_split = line.split_whitespace();

            let niks3_name = niks3_list_split.next().unwrap().to_string();
            if !niks3_name.starts_with("CI.") {
                println!("INFO  : {niks3_name}");
                continue;
            }
            let id_split: Vec<&str> = niks3_name.split(".").collect();
            let split_len = niks3_name.len();
            let repository;
            if split_len >= 2 {
                repository = format!("{}.{}", id_split[1], id_split[2]);
            } else {
                repository = "".to_string()
            }
            let branch = id_split.get(3).map(|x| x.to_string()).unwrap_or_default();
            let name = id_split.get(4).map(|x| x.to_string()).unwrap_or_default();

            let store_hash = niks3_list_split.next().unwrap().to_string();

            let date: DateTime<Local> =
                DateTime::parse_from_rfc3339(niks3_list_split.next().unwrap())
                    .unwrap()
                    .into();

            let deriv = deriv::Deriv {
                name,
                repo: repository,
                storeHash: store_hash,
                branch,
                force: None,
                date_added: Some(date),
            };
            outputs.push(deriv);
        }
        Some(outputs)
    }

    pub fn delete(name: &String, branch: &String) -> DBResponse {
        todo!()
    }
    pub fn query(name: String, branch: String) -> Vec<deriv::Deriv> {
        println!("INFO: name: \"{name}\", branch: \"{branch}\"");
        Self::get_all()
            .unwrap()
            .into_iter()
            .filter(|d| {
                (name.as_str() == "_" || d.name == name)
                    && (branch.as_str() == "_" || d.branch == branch)
            })
            .collect()
    }
}

struct DBResponse {
    message: String,
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
