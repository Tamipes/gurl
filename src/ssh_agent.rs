use std::{
    collections::HashMap,
    io::Write,
    process::{Command, Stdio},
};

use colored::Colorize;

use crate::print_exit;

pub struct SshAgent {
    envs: HashMap<String, String>,
}

impl SshAgent {
    pub fn new(ssh_key: String) -> Result<SshAgent, std::io::Error> {
        let mut sruct: SshAgent = SshAgent {
            envs: HashMap::new(),
        };

        let agent = Command::new("ssh-agent")
            .stdout(Stdio::piped())
            .output()
            .expect("failed to run `ssh-agent`");
        for line in String::from_utf8_lossy(&agent.stdout).lines() {
            if let Some(rest) = line.strip_prefix("SSH_AUTH_SOCK=") {
                sruct.envs.insert(
                    "SSH_AUTH_SOCK".to_string(),
                    rest.split(';').next().unwrap().to_string(),
                );
            }

            if let Some(rest) = line.strip_prefix("SSH_AGENT_PID=") {
                sruct.envs.insert(
                    "SSH_AGENT_PID".to_string(),
                    rest.split(';').next().unwrap().to_string(),
                );
            }
        }

        match sruct.envs.get("SSH_AGENT_PID") {
            Some(str) => println!("INFO: ssh-agent pid: {}", str),
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "`ssh-agent` failed to run correctly. - Tami",
                ));
            }
        }
        if !agent.status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Creating an `ssh-agent` returned a non-zero exit code. - Tami",
            ));
        }

        let mut ssh_add = Command::new("ssh-add")
            .arg("-")
            .envs(&sruct.envs)
            .stdin(Stdio::piped())
            .spawn()?;

        let ssh_add_input = match ssh_add.stdin.as_mut() {
            Some(x) => x,
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Could not get mutable stdin of ssh-add. - Tami",
                ))
            }
        };

        ssh_add_input.write_all(ssh_key.as_bytes())?;
        ssh_add_input.write_all(b"\n")?;
        drop(ssh_add.stdin.take());

        if !ssh_add.wait_with_output()?.status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Running a `ssh-add` returned a non-zero exit code. - Tami",
            ));
        }
        return Ok(sruct);
    }

    pub fn add_ssh_opts(&mut self, str: String) {
        self.envs.insert("NIX_SSHOPTS".to_string(), str);
    }

    pub fn run_cmd(&mut self, cmd: &mut Command) -> Result<std::process::Output, std::io::Error> {
        cmd.envs(&self.envs).output()
    }
}

impl Drop for SshAgent {
    fn drop(&mut self) {
        match self.envs.get("SSH_AGENT_PID") {
            Some(_) => {
                let agent_close = match Command::new("ssh-agent")
                    .arg("-k")
                    .envs(&self.envs)
                    .output()
                {
                    Ok(x) => x,
                    Err(x) => {
                        print_exit(
                            &format!(
                                "ERROR: {}",
                                "could not close ssh-agent when dropping SshAgent. - Tami".red()
                            ),
                            1,
                        );
                        return;
                    }
                };
                if !agent_close.status.success() {
                    println!("WARN: Could not close `ssh-agent`");
                    for (key, value) in &self.envs {
                        println!("\t{key}={value}; export {key};");
                    }
                }
            }
            None => print_exit(
                &format!(
                    "ERROR: {}",
                    "could not close ssh-agent when dropping SshAgent(no PID). - Tami".red()
                ),
                2,
            ),
        };
    }
}

// pub fn run_in_ssh_agent(
//     ssh_key: String,
//     cmd: &mut Command,
// ) -> Result<std::process::Output, std::io::Error> {
//     // let mut envs = HashMap::new();

//     // let agent = Command::new("ssh-agent")
//     //     .stdout(Stdio::piped())
//     //     .output()
//     //     .expect("failed to run `ssh-agent`");
//     // for line in String::from_utf8_lossy(&agent.stdout).lines() {
//     //     if let Some(rest) = line.strip_prefix("SSH_AUTH_SOCK=") {
//     //         envs.insert(
//     //             "SSH_AUTH_SOCK".to_string(),
//     //             rest.split(';').next().unwrap().to_string(),
//     //         );
//     //     }

//     //     if let Some(rest) = line.strip_prefix("SSH_AGENT_PID=") {
//     //         envs.insert(
//     //             "SSH_AGENT_PID".to_string(),
//     //             rest.split(';').next().unwrap().to_string(),
//     //         );
//     //     }
//     // }
//     // match envs.get("SSH_AGENT_PID") {
//     //     Some(str) => println!("INFO: ssh-agent pid: {}", str),
//     //     None => {
//     //         return Err(std::io::Error::new(
//     //             std::io::ErrorKind::Other,
//     //             "`ssh-agent` failed to run correctly. - Tami",
//     //         ));
//     //     }
//     // }
//     // if !agent.status.success() {
//     //     return Err(std::io::Error::new(
//     //         std::io::ErrorKind::Other,
//     //         "Creating an `ssh-agent` returned a non-zero exit code. - Tami",
//     //     ));
//     // }

//     let output = inner_ssh_agent(ssh_key, cmd, &envs);

//     // let agent_close = Command::new("ssh-agent").arg("-k").envs(&envs).output()?;

//     // if !agent_close.status.success() {
//     //     println!("WARN: Could not close `ssh-agent`");
//     //     for (key, value) in envs {
//     //         println!("\t{key}={value}; export {key};");
//     //     }
//     // }

//     return output;
// }

// pub fn inner_ssh_agent(
//     ssh_key: String,
//     cmd: &mut Command,
//     envs: &HashMap<String, String>,
// ) -> Result<std::process::Output, std::io::Error> {
//     // let mut ssh_add = Command::new("ssh-add")
//     //     .arg("-")
//     //     .envs(envs.clone())
//     //     .stdin(Stdio::piped())
//     //     .spawn()?;

//     // let ssh_add_input = match ssh_add.stdin.as_mut() {
//     //     Some(x) => x,
//     //     None => {
//     //         return Err(std::io::Error::new(
//     //             std::io::ErrorKind::Other,
//     //             "Could not get mutable stdin of ssh-add. - Tami",
//     //         ))
//     //     }
//     // };

//     // ssh_add_input.write_all(ssh_key.as_bytes())?;
//     // ssh_add_input.write_all(b"\n")?;
//     // drop(ssh_add.stdin.take());

//     // if !ssh_add.wait_with_output()?.status.success() {
//     //     return Err(std::io::Error::new(
//     //         std::io::ErrorKind::Other,
//     //         "Running a `ssh-add` returned a non-zero exit code. - Tami",
//     //     ));
//     // }

//     return cmd.envs(envs).output();
// }
