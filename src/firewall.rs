use std::{collections::BTreeSet, net::Ipv4Addr};

use crate::data::LoadedConfig;

pub const RULE_GROUP: &str = "CS2_SERVER_CHOOSER";

#[cfg(target_os = "linux")]
pub const FIREWALL_OBJECT: &str = RULE_GROUP;
#[cfg(target_os = "windows")]
pub const FIREWALL_OBJECT: &str = "CS2 Server Chooser";
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub const FIREWALL_OBJECT: &str = RULE_GROUP;

pub const CHAIN: &str = FIREWALL_OBJECT;

#[cfg(target_os = "linux")]
const COMMENT: &str = "cs2-server-chooser owned jump";
const WINDOWS_RULE_DESCRIPTION: &str = "cs2-server-chooser blocked relay";

#[derive(Clone)]
pub enum FirewallAction {
    Apply(FirewallPlan),
    Clear,
}

impl FirewallAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Apply(_) => apply_action_label(),
            Self::Clear => clear_action_label(),
        }
    }

    pub fn run(&self, elevation_password: Option<&str>) -> Result<String, String> {
        match self {
            Self::Apply(plan) => {
                let count = plan.apply(elevation_password)?;
                Ok(format!("Applied {count} current relay blocks with {FIREWALL_OBJECT}"))
            }
            Self::Clear => {
                FirewallPlan::clear(elevation_password)?;
                Ok(format!("Removed {FIREWALL_OBJECT} firewall rules"))
            }
        }
    }
}

#[derive(Clone)]
pub struct FirewallPlan {
    rules: Vec<Rule>,
}

// Compatibility alias for older app.rs imports.
pub type IptablesPlan = FirewallPlan;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rule {
    relay: Ipv4Addr,
    start_port: u16,
    end_port: u16,
}

impl FirewallPlan {
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

    fn apply(&self, elevation_password: Option<&str>) -> Result<usize, String> {
        platform_apply(&self.rules, elevation_password)
    }

    fn clear(elevation_password: Option<&str>) -> Result<(), String> {
        platform_clear(elevation_password)
    }
}

pub fn firewall_backend_name() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "iptables"
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Defender Firewall"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "unsupported firewall"
    }
}

pub fn firewall_apply_warning() -> String {
    #[cfg(target_os = "linux")]
    {
        format!(
            "Selected POPs are allowed; unselected POPs are blocked. This only modifies the {FIREWALL_OBJECT} chain plus its owned OUTPUT jump."
        )
    }
    #[cfg(target_os = "windows")]
    {
        format!(
            "Selected POPs are allowed; unselected POPs are blocked. This modifies outbound Windows Defender Firewall rules named {FIREWALL_OBJECT}. Run the app as Administrator."
        )
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "This platform is not supported for automatic firewall changes.".to_owned()
    }
}

pub fn elevation_prompt_label() -> Option<&'static str> {
    #[cfg(target_os = "linux")]
    {
        Some("Sudo password")
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn primary_run_button_label() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Run"
    }
    #[cfg(target_os = "windows")]
    {
        "Run (requires Administrator)"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "Run"
    }
}

pub fn cached_elevation_button_label() -> Option<&'static str> {
    #[cfg(target_os = "linux")]
    {
        Some("Use cached sudo")
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

fn apply_action_label() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Apply iptables rules"
    }
    #[cfg(target_os = "windows")]
    {
        "Apply Windows Firewall rules"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "Apply firewall rules"
    }
}

fn clear_action_label() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "Remove chooser iptables rules"
    }
    #[cfg(target_os = "windows")]
    {
        "Remove chooser Windows Firewall rules"
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        "Remove chooser firewall rules"
    }
}

#[cfg(target_os = "linux")]
fn platform_apply(rules: &[Rule], sudo_password: Option<&str>) -> Result<usize, String> {
    linux::ensure_chain(sudo_password)?;
    linux::run_iptables(sudo_password, &["-F", RULE_GROUP])?;
    for rule in rules {
        let port_range = format!("{}:{}", rule.start_port, rule.end_port);
        let dest = rule.relay.to_string();
        linux::run_iptables(sudo_password, &linux_rule_args(&dest, &port_range))?;
    }
    Ok(rules.len())
}

#[cfg(target_os = "linux")]
fn platform_clear(sudo_password: Option<&str>) -> Result<(), String> {
    linux::clear(sudo_password)
}

#[cfg(target_os = "windows")]
fn platform_apply(rules: &[Rule], _elevation_password: Option<&str>) -> Result<usize, String> {
    // netsh cannot safely update an existing set in place, so recreate our
    // named outbound rules each time. Windows elevation must be handled by
    // launching this app as Administrator; there is no sudo-equivalent stdin
    // password flow.
    let _ = windows::run_netsh(&windows_delete_rule_args());
    for rule in rules {
        let remote_ip = rule.relay.to_string();
        let remote_port = windows_port_range(rule.start_port, rule.end_port);
        windows::run_netsh(&windows_add_rule_args(&remote_ip, &remote_port))?;
    }
    Ok(rules.len())
}

#[cfg(target_os = "windows")]
fn platform_clear(_elevation_password: Option<&str>) -> Result<(), String> {
    windows::run_netsh(&windows_delete_rule_args()).map(|_| ())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_apply(_rules: &[Rule], _elevation_password: Option<&str>) -> Result<usize, String> {
    Err("Automatic firewall changes are only supported on Linux and Windows".to_owned())
}
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn platform_clear(_elevation_password: Option<&str>) -> Result<(), String> {
    Err("Automatic firewall changes are only supported on Linux and Windows".to_owned())
}

#[cfg(target_os = "linux")]
fn linux_rule_args<'a>(dest: &'a str, port_range: &'a str) -> [&'a str; 16] {
    [
        "-A",
        RULE_GROUP,
        "-p",
        "udp",
        "-m",
        "udp",
        "-d",
        dest,
        "--dport",
        port_range,
        "-m",
        "comment",
        "--comment",
        "cs2-server-chooser blocked relay",
        "-j",
        "REJECT",
    ]
}

fn windows_port_range(start_port: u16, end_port: u16) -> String {
    if start_port == end_port {
        start_port.to_string()
    } else {
        format!("{start_port}-{end_port}")
    }
}

fn windows_delete_rule_args() -> Vec<String> {
    vec![
        "advfirewall".to_owned(),
        "firewall".to_owned(),
        "delete".to_owned(),
        "rule".to_owned(),
        format!("name={FIREWALL_OBJECT}"),
    ]
}

fn windows_add_rule_args(remote_ip: &str, remote_port: &str) -> Vec<String> {
    vec![
        "advfirewall".to_owned(),
        "firewall".to_owned(),
        "add".to_owned(),
        "rule".to_owned(),
        format!("name={FIREWALL_OBJECT}"),
        "dir=out".to_owned(),
        "action=block".to_owned(),
        "protocol=UDP".to_owned(),
        format!("remoteip={remote_ip}"),
        format!("remoteport={remote_port}"),
        format!("description={WINDOWS_RULE_DESCRIPTION}"),
    ]
}

#[cfg(target_os = "linux")]
mod linux {
    use std::{io::Write, process::{Command, Stdio}};

    use super::{COMMENT, RULE_GROUP};

    pub(super) fn ensure_chain(sudo_password: Option<&str>) -> Result<(), String> {
        if !iptables_success(sudo_password, &["-S", RULE_GROUP])? {
            run_iptables(sudo_password, &["-N", RULE_GROUP])?;
        }

        let uid = current_uid();
        let uid_text = uid.to_string();
        let jump = output_jump_args(&uid_text);
        if !run_iptables_success(sudo_password, "-C", "OUTPUT", &jump)? {
            run_iptables_with_chain(sudo_password, "-A", "OUTPUT", &jump)?;
        }
        Ok(())
    }

    pub(super) fn clear(sudo_password: Option<&str>) -> Result<(), String> {
        let uid = current_uid();
        let uid_text = uid.to_string();
        let jump = output_jump_args(&uid_text);
        while run_iptables_success(sudo_password, "-D", "OUTPUT", &jump)? {}
        let _ = run_iptables(sudo_password, &["-F", RULE_GROUP]);
        let _ = run_iptables(sudo_password, &["-X", RULE_GROUP]);
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
            RULE_GROUP,
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

    pub(super) fn run_iptables(sudo_password: Option<&str>, args: &[&str]) -> Result<(), String> {
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
}

#[cfg(target_os = "windows")]
mod windows {
    use std::process::{Command, Stdio};

    pub(super) fn run_netsh(args: &[String]) -> Result<(), String> {
        let output = Command::new("netsh")
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|err| err.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let details = if !stderr.trim().is_empty() {
                stderr.trim()
            } else {
                stdout.trim()
            };
            Err(format!(
                "netsh exited with {}{}",
                output.status,
                if details.is_empty() {
                    String::new()
                } else {
                    format!(": {details}")
                }
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netsh_port_range_uses_windows_separator() {
        assert_eq!(windows_port_range(27015, 27050), "27015-27050");
        assert_eq!(windows_port_range(27015, 27015), "27015");
    }

    #[test]
    fn netsh_add_rule_contains_expected_outbound_udp_filters() {
        let args = windows_add_rule_args("1.2.3.4", "27015-27050");
        assert!(args.contains(&"dir=out".to_owned()));
        assert!(args.contains(&"action=block".to_owned()));
        assert!(args.contains(&"protocol=UDP".to_owned()));
        assert!(args.contains(&"remoteip=1.2.3.4".to_owned()));
        assert!(args.contains(&"remoteport=27015-27050".to_owned()));
    }
}
