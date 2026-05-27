use std::{
    collections::BTreeSet,
    io::Write,
    net::Ipv4Addr,
    process::{Command, Stdio},
};

use crate::data::LoadedConfig;

pub const CHAIN: &str = "CS2_SERVER_CHOOSER";
const COMMENT: &str = "cs2-server-chooser owned jump";

#[derive(Clone)]
pub enum FirewallAction {
    Apply(IptablesPlan),
    Clear,
}

impl FirewallAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Apply(_) => "Apply iptables rules",
            Self::Clear => "Remove chooser iptables rules",
        }
    }

    pub fn run(&self, sudo_password: Option<&str>) -> Result<String, String> {
        match self {
            Self::Apply(plan) => {
                let count = plan.apply(sudo_password)?;
                Ok(format!("Applied {count} current relay blocks in {CHAIN}"))
            }
            Self::Clear => {
                IptablesPlan::clear(sudo_password)?;
                Ok(format!("Removed {CHAIN} jump and chain"))
            }
        }
    }
}

#[derive(Clone)]
pub struct IptablesPlan {
    rules: Vec<Rule>,
}

#[derive(Clone)]
struct Rule {
    relay: Ipv4Addr,
    start_port: u16,
    end_port: u16,
}

impl IptablesPlan {
    pub fn from_config(config: &LoadedConfig, selected: &BTreeSet<&str>) -> Self {
        let mut rules = Vec::new();
        for pop in &config.pops {
            if selected.contains(pop.code.as_str()) {
                continue;
            }
            for relay in &pop.relays {
                rules.push(Rule {
                    relay: relay.ipv4,
                    start_port: relay.port_range[0],
                    end_port: relay.port_range[1],
                });
            }
        }
        Self { rules }
    }

    fn apply(&self, sudo_password: Option<&str>) -> Result<usize, String> {
        ensure_chain(sudo_password)?;
        run_iptables(sudo_password, &["-F", CHAIN])?;
        for rule in &self.rules {
            let port_range = format!("{}:{}", rule.start_port, rule.end_port);
            let dest = rule.relay.to_string();
            run_iptables(
                sudo_password,
                &[
                    "-A",
                    CHAIN,
                    "-p",
                    "udp",
                    "-m",
                    "udp",
                    "-d",
                    &dest,
                    "--dport",
                    &port_range,
                    "-m",
                    "comment",
                    "--comment",
                    "cs2-server-chooser blocked relay",
                    "-j",
                    "REJECT",
                ],
            )?;
        }
        Ok(self.rules.len())
    }

    fn clear(sudo_password: Option<&str>) -> Result<(), String> {
        let uid = current_uid();
        let uid_text = uid.to_string();
        let jump = output_jump_args(&uid_text);
        while run_iptables_success(sudo_password, "-D", "OUTPUT", &jump)? {}
        let _ = run_iptables(sudo_password, &["-F", CHAIN]);
        let _ = run_iptables(sudo_password, &["-X", CHAIN]);
        Ok(())
    }
}

fn ensure_chain(sudo_password: Option<&str>) -> Result<(), String> {
    if !iptables_success(sudo_password, &["-S", CHAIN])? {
        run_iptables(sudo_password, &["-N", CHAIN])?;
    }

    let uid = current_uid();
    let uid_text = uid.to_string();
    let jump = output_jump_args(&uid_text);
    if !run_iptables_success(sudo_password, "-C", "OUTPUT", &jump)? {
        run_iptables_with_chain(sudo_password, "-A", "OUTPUT", &jump)?;
    }
    Ok(())
}

fn output_jump_args(uid_text: &str) -> [&str; 14] {
    [
        "-p",
        "udp",
        "-m",
        "udp",
        "-m",
        "owner",
        "--uid-owner",
        uid_text,
        "-m",
        "comment",
        "--comment",
        COMMENT,
        "-j",
        CHAIN,
    ]
}

fn current_uid() -> u32 {
    std::env::var("UID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| {
            let output = Command::new("id")
                .arg("-u")
                .output()
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .unwrap_or_else(|| "0".to_owned());
            output.trim().parse().unwrap_or(0)
        })
}

fn iptables_success(sudo_password: Option<&str>, args: &[&str]) -> Result<bool, String> {
    run_sudo(sudo_password, "iptables", args).map(|output| output.status.success())
}

fn run_iptables_success(
    sudo_password: Option<&str>,
    action: &str,
    chain: &str,
    args: &[&str],
) -> Result<bool, String> {
    let mut all_args = vec![action, chain];
    all_args.extend_from_slice(args);
    iptables_success(sudo_password, &all_args)
}

fn run_iptables_with_chain(
    sudo_password: Option<&str>,
    action: &str,
    chain: &str,
    args: &[&str],
) -> Result<(), String> {
    let mut all_args = vec![action, chain];
    all_args.extend_from_slice(args);
    run_iptables(sudo_password, &all_args)
}

fn run_iptables(sudo_password: Option<&str>, args: &[&str]) -> Result<(), String> {
    let output = run_sudo(sudo_password, "iptables", args)?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "iptables exited with {}{}",
            output.status,
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        ))
    }
}

fn run_sudo(
    sudo_password: Option<&str>,
    program: &str,
    args: &[&str],
) -> Result<std::process::Output, String> {
    let mut command = Command::new("sudo");
    if sudo_password.is_some() {
        command.args(["-S", "-p", ""]);
    } else {
        command.arg("-n");
    }
    command.arg(program).args(args).stderr(Stdio::piped());

    if let Some(password) = sudo_password {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|err| err.to_string())?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(format!("{password}\n").as_bytes())
                .map_err(|err| err.to_string())?;
        }
        child.wait_with_output().map_err(|err| err.to_string())
    } else {
        command.output().map_err(|err| err.to_string())
    }
}
