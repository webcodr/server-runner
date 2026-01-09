use crate::config::OutputConfig;
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::thread;

pub struct ServerProcess {
    pub name: String,
    pub process: Child,
}

pub fn run_command(
    command: &str,
    env_vars: &HashMap<String, String>,
    output_config: &OutputConfig,
) -> anyhow::Result<Child> {
    let command_parts =
        shlex::split(command).ok_or_else(|| anyhow::anyhow!("Invalid command: {}", command))?;

    if command_parts.is_empty() {
        bail!("Empty command provided");
    }

    let mut cmd = Command::new(&command_parts[0]);

    for part in command_parts.iter().skip(1) {
        cmd.arg(part);
    }

    // Add environment variables
    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    // Configure output handling
    match output_config.mode.as_str() {
        "inherit" => {
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
        }
        "null" => {
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());
        }
        "capture" => {
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());
        }
        "file" => {
            if let Some(stdout_path) = &output_config.stdout {
                let stdout_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(stdout_path)
                    .context(format!("Could not open stdout file: {}", stdout_path))?;
                cmd.stdout(stdout_file);
            } else {
                cmd.stdout(Stdio::null());
            }

            if let Some(stderr_path) = &output_config.stderr {
                let stderr_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(stderr_path)
                    .context(format!("Could not open stderr file: {}", stderr_path))?;
                cmd.stderr(stderr_file);
            } else {
                cmd.stderr(Stdio::null());
            }
        }
        _ => {
            cmd.stdout(Stdio::inherit());
            cmd.stderr(Stdio::inherit());
        }
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }

    let mut child = cmd.spawn()?;

    // If in capture mode with prefix, spawn threads to handle output
    if output_config.mode == "capture" {
        if let Some(prefix) = &output_config.prefix {
            let prefix_clone = prefix.clone();
            if let Some(stdout) = child.stdout.take() {
                thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            println!("{} {}", prefix_clone, line);
                        }
                    }
                });
            }

            let prefix_clone = prefix.clone();
            if let Some(stderr) = child.stderr.take() {
                thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        if let Ok(line) = line {
                            eprintln!("{} {}", prefix_clone, line);
                        }
                    }
                });
            }
        }
    }

    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_simple_command() {
        let env = HashMap::new();
        let output = OutputConfig::default();

        let mut child = run_command("echo test", &env, &output).unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_run_command_with_env() {
        let mut env = HashMap::new();
        env.insert("TEST_VAR".to_string(), "test_value".to_string());

        let output = OutputConfig {
            mode: "null".to_string(),
            stdout: None,
            stderr: None,
            prefix: None,
        };

        #[cfg(unix)]
        let command = "sh -c 'echo $TEST_VAR'";
        #[cfg(windows)]
        let command = "cmd /c echo %TEST_VAR%";

        let mut child = run_command(command, &env, &output).unwrap();
        let status = child.wait().unwrap();
        assert!(status.success());
    }
}
