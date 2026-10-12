use chrono::{DateTime, Local};
use colored::Colorize;
use serde_derive::{Deserialize, Serialize};
use std::fmt::Debug;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::{handle_date_to_dynamic_info, print_exit, ssh_agent, DBResponse, DB};

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
pub struct Deriv {
    pub repo: String,
    pub name: String,
    pub storeHash: String,
    pub branch: String,
    pub force: Option<bool>,
    pub date_added: Option<DateTime<Local>>,
}

pub fn handle_deriv_del(branch: String, name: String) {
    fn print_res(res: DBResponse, name: String, branch: String) -> bool {
        if res.status.success() {
            println!("{} on the branch {}: {}", name, branch, res.message.green());
        } else {
            println!(
                "ERROR: \"{}\" on the branch \"{}\": {}",
                name,
                branch,
                res.message.red()
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

pub fn handle_deriv_ls() {
    let derivations = DB::get_all().unwrap();
    let current_system = fs::read_link("/run/current-system");
    match current_system {
        Ok(x) => match x.into_os_string().into_string() {
            Ok(x) => crate::pretty_print(derivations, x.as_str()),
            Err(x) => {
                println!(
                    "ERROR: {}",
                    format!(
                        "failed parsing current system's nix store hash(OsString) to String {:?}",
                        x
                    )
                    .red()
                );
                crate::pretty_print(derivations, "");
                std::process::exit(1);
            }
        },
        Err(x) => {
            println!(
                "ERROR: {}",
                format!("getting the current system's store hash: {:?}", x).red()
            );
            crate::pretty_print(derivations, "");
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
pub fn handle_deriv_upload(name: &str, hash: &str, branch: Option<String>, force: Option<bool>) {
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
                let known_hosts_file = std::env::var("GURL_SSH_HOSTS");

                let out;

                if let Ok(priv_key) = private_key {
                    println!("INFO: using ssh-agent and private key");

                    let mut agent =
                        ssh_agent::SshAgent::new(priv_key).expect("Failed to create SshAgent");
                    if let Ok(file_path) = known_hosts_file {
                        println!("INFO: using known_hosts file");
                        agent.add_ssh_opts(format!("-o UserKnownHostsFile={}", file_path))
                    }

                    out = agent.run_cmd(
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
enum UploadReqError {
    Comment(String),
    StoreHashNotFound,
}

fn make_upload_req(json_payload: String) -> Result<String, UploadReqError> {
    match crate::make_req("POST /derivations", Some(json_payload.as_str())) {
        Ok(res) => {
            if res.status.success() {
                Ok(format!(
                    "{} (server response: {})",
                    "Derivation uploaded succesffuly!".green(),
                    res.message
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
                        format!("\t{}", res.message)
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

pub fn handle_deriv_apply(name: String, branch: String) {
    let name = if name == "$HOSTNAME" {
        String::from_utf8_lossy(
            &Command::new("hostname")
                .output()
                .expect("Could not run `hostname`(unix? command), consider adding --name manually.")
                .stdout
                .trim_ascii_end(),
        )
        .into_owned()
    } else {
        name
    };
    println!("INFO: name set as: {}", name);

    let deriv = DB::query(name, branch)
        .into_iter()
        .next()
        .expect("There was no match for this name and branch configuration!");

    println!("INFO: this will be installed:");
    println!("\tname: {}", deriv.name);
    println!("\tbranch: {}", deriv.branch);
    println!("\thash: {}", deriv.storeHash);
    println!("\tdate: {}", handle_date_to_dynamic_info(deriv.date_added));

    let password = sudo_password_getter().expect("Failed to get sudo password");

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
fn sudo_password_getter() -> Option<String> {
    let password = rpassword::prompt_password("[sudo] password for later: ").unwrap();

    // Check if password works with sudo
    let mut cmd = Command::new("sudo")
        .args(vec![
            "-S",
            "echo",
            "-e",
            &format!("\n{}", "INFO: Succesfully acquired sudo password!".green()),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Some error with starting sudo echo checker");
    let mut stdin = cmd
        .stdin
        .take()
        .expect("Failed to open stdin of sudo echo checker");
    let pswd = password.clone();
    std::thread::spawn(move || stdin.write_all(pswd.as_bytes()));
    let status = cmd.wait();
    match status {
        Ok(es) => {
            if !es.success() {
                println!(
                    "ERROR: {}",
                    "Bad password for sudo. Sudo check failed.".red()
                );
                return None;
            } else {
                return Some(password);
            }
        }
        Err(x) => {
            println!(
                "ERROR: {}",
                "Error during checking exit code of sudo echo checker".red()
            );
            return None;
        }
    }
}
pub fn handle_deriv_reapply() {
    let password = sudo_password_getter().expect("Failed to get sudo password");
    let mut cmd = Command::new("sudo")
        .args(vec![
            "-S",
            "/nix/var/nix/profiles/system/bin/switch-to-configuration",
            "switch",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Some error with starting sudo switch-to-configuration");
    let mut stdin = cmd.stdin.take().expect("Failed to open stdin");
    let pswd = password.clone();
    std::thread::spawn(move || stdin.write_all(pswd.as_bytes()));
    let status = cmd.wait();
    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!(
                    "INFO: {}",
                    "Successfully reapplied the configuration!".green()
                );
            } else {
                println!("ERROR: {}", "Failed to run switch-to-configuration!".red());
                return;
            }
        }
        Err(x) => {
            println!(
                "ERROR: {}",
                "Failed to start sudo switch-to-configuration".red()
            );
            return;
        }
    }
}

pub fn handle_deriv_rollback() {
    let password = sudo_password_getter().expect("Failed to get sudo password");

    let mut cmd = Command::new("sudo")
        .args(vec![
            "-S",
            "nix-env",
            "--profile",
            "/nix/var/nix/profiles/system",
            "--rollback",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Some error with starting sudo nix-env");
    let mut stdin = cmd.stdin.take().expect("Failed to open stdin");
    let pswd = password.clone();
    std::thread::spawn(move || stdin.write_all(pswd.as_bytes()));
    let status = cmd.wait();
    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!("INFO: {}", "Successfully rolled back profile!");
            } else {
                println!(
                    "ERROR: {}",
                    "Failed to roll back profile with nix-env!".red()
                );
                return;
            }
        }
        Err(x) => {
            println!("ERROR: {}", "Failed to start sudo nix-env".red());
            return;
        }
    }

    let mut cmd = Command::new("sudo")
        .args(vec![
            "-S",
            "/nix/var/nix/profiles/system/bin/switch-to-configuration",
            "switch",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("Some error with starting sudo switch-to-configuration");
    let mut stdin = cmd.stdin.take().expect("Failed to open stdin");
    let pswd = password.clone();
    std::thread::spawn(move || stdin.write_all(pswd.as_bytes()));
    let status = cmd.wait();
    match status {
        Ok(exit_status) => {
            if exit_status.success() {
                println!(
                    "INFO: {}",
                    "Successfully applied(switched) to the old configuration!".green()
                );
            } else {
                println!(
                    "ERROR: {}",
                    "Failed to apply(switch) back to the old profile with switch-to-configuration!"
                        .red()
                );
                return;
            }
        }
        Err(x) => {
            println!(
                "ERROR: {}",
                "Failed to start sudo switch-to-configuration".red()
            );
            return;
        }
    }
}
