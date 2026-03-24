// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Standalone gateway binary.
//!
//! Boots a libkrun microVM running the OpenShell control plane (k3s +
//! openshell-server). By default it uses the pre-built rootfs at
//! `~/.local/share/openshell/gateway/rootfs`.
//!
//! # Codesigning (macOS)
//!
//! This binary must be codesigned with the `com.apple.security.hypervisor`
//! entitlement. See `entitlements.plist` in this crate.
//!
//! ```sh
//! codesign --entitlements crates/openshell-vm/entitlements.plist --force -s - target/debug/gateway
//! ```

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::{Parser, Subcommand, ValueHint};

/// Boot the OpenShell gateway microVM.
///
/// Starts a libkrun microVM running a k3s Kubernetes cluster with the
/// OpenShell control plane. Use `--exec` to run a custom process instead.
#[derive(Parser)]
#[command(name = "gateway", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<GatewayCommand>,

    #[command(flatten)]
    run: RunArgs,
}

#[derive(Subcommand)]
enum GatewayCommand {
    /// Run a command with the gateway kubeconfig pre-configured.
    ///
    /// Examples:
    ///   gateway exec -- kubectl get pods -A
    ///   gateway exec -- kubectl -n openshell logs statefulset/openshell
    ///   gateway exec -- sh
    Exec {
        /// Command and arguments to run on the host with KUBECONFIG pointing
        /// at the VM-backed gateway cluster.
        #[arg(trailing_var_arg = true, required = true)]
        command: Vec<String>,
    },
}

#[derive(clap::Args)]
struct RunArgs {
    /// Path to the rootfs directory (aarch64 Linux).
    /// Defaults to `~/.local/share/openshell/gateway/rootfs`.
    #[arg(long, value_hint = ValueHint::DirPath)]
    rootfs: Option<PathBuf>,

    /// Executable path inside the VM. When set, runs this instead of
    /// the default k3s server.
    #[arg(long)]
    exec: Option<String>,

    /// Arguments to the executable (requires `--exec`).
    #[arg(long, num_args = 1..)]
    args: Vec<String>,

    /// Environment variables in `KEY=VALUE` form (requires `--exec`).
    #[arg(long, num_args = 1..)]
    env: Vec<String>,

    /// Working directory inside the VM.
    #[arg(long, default_value = "/")]
    workdir: String,

    /// Port mappings (`host_port:guest_port`).
    #[arg(long, short, num_args = 1..)]
    port: Vec<String>,

    /// Number of virtual CPUs (default: 4 for gateway, 2 for --exec).
    #[arg(long)]
    vcpus: Option<u8>,

    /// RAM in MiB (default: 8192 for gateway, 2048 for --exec).
    #[arg(long)]
    mem: Option<u32>,

    /// libkrun log level (0=Off .. 5=Trace).
    #[arg(long, default_value_t = 1)]
    krun_log_level: u32,

    /// Networking backend: "gvproxy" (default), "tsi", or "none".
    #[arg(long, default_value = "gvproxy")]
    net: String,
}

fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    let code = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    };

    if code != 0 {
        std::process::exit(code);
    }
}

fn run(cli: Cli) -> Result<i32, Box<dyn std::error::Error>> {
    if let Some(command) = cli.command {
        return match command {
            GatewayCommand::Exec { command } => exec_with_gateway_kubeconfig(&command),
        };
    }

    let cli = cli.run;

    let net_backend = match cli.net.as_str() {
        "tsi" => openshell_vm::NetBackend::Tsi,
        "none" => openshell_vm::NetBackend::None,
        "gvproxy" => openshell_vm::NetBackend::Gvproxy {
            binary: openshell_vm::default_runtime_gvproxy_path(),
        },
        other => {
            return Err(
                format!("unknown --net backend: {other} (expected: gvproxy, tsi, none)").into(),
            );
        }
    };

    let rootfs = match cli.rootfs {
        Some(p) => p,
        None => openshell_bootstrap::paths::default_rootfs_dir()?,
    };

    let mut config = if let Some(exec_path) = cli.exec {
        openshell_vm::VmConfig {
            rootfs,
            vcpus: cli.vcpus.unwrap_or(2),
            mem_mib: cli.mem.unwrap_or(2048),
            exec_path,
            args: cli.args,
            env: cli.env,
            workdir: cli.workdir,
            port_map: cli.port,
            vsock_ports: vec![],
            log_level: cli.krun_log_level,
            console_output: None,
            net: net_backend.clone(),
        }
    } else {
        let mut c = openshell_vm::VmConfig::gateway(rootfs);
        if !cli.port.is_empty() {
            c.port_map = cli.port;
        }
        if let Some(v) = cli.vcpus {
            c.vcpus = v;
        }
        if let Some(m) = cli.mem {
            c.mem_mib = m;
        }
        c.net = net_backend;
        c
    };
    config.log_level = cli.krun_log_level;

    Ok(openshell_vm::launch(&config)?)
}

fn gateway_kubeconfig_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    Ok(PathBuf::from(home).join(".kube").join("gateway.yaml"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn openshell_kubectl_wrapper_path() -> PathBuf {
    workspace_root().join("scripts/bin/kubectl")
}

fn is_openshell_kubectl_wrapper(path: &Path) -> bool {
    path.canonicalize().ok() == openshell_kubectl_wrapper_path().canonicalize().ok()
}

fn filtered_path() -> OsString {
    let wrapper_dir = openshell_kubectl_wrapper_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let entries = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|entry| entry != &wrapper_dir)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    std::env::join_paths(entries).unwrap_or_else(|_| OsString::from("/usr/bin:/bin"))
}

fn resolve_kubectl_binary() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(path) = std::env::var_os("OPENSHELL_GATEWAY_KUBECTL") {
        return Ok(PathBuf::from(path));
    }

    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("kubectl");
        if candidate.is_file() && !is_openshell_kubectl_wrapper(&candidate) {
            return Ok(candidate);
        }
    }

    Err(
        "could not find a real kubectl binary on PATH; install kubectl or set OPENSHELL_GATEWAY_KUBECTL"
            .into(),
    )
}

fn configure_clean_env(cmd: &mut Command, kubeconfig: &Path) {
    cmd.env_clear().env("KUBECONFIG", kubeconfig);

    for key in [
        "HOME",
        "TERM",
        "COLORTERM",
        "NO_COLOR",
        "LANG",
        "LC_ALL",
        "LC_CTYPE",
        "TMPDIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            cmd.env(key, value);
        }
    }

    cmd.env("PATH", filtered_path());
}

fn exec_with_gateway_kubeconfig(command: &[String]) -> Result<i32, Box<dyn std::error::Error>> {
    let kubeconfig = gateway_kubeconfig_path()?;
    if !kubeconfig.is_file() {
        return Err(format!(
            "gateway kubeconfig not found: {}\nStart the VM first with `gateway` or `mise run vm`.",
            kubeconfig.display()
        )
        .into());
    }

    let program = &command[0];
    let mut cmd = if program == "kubectl" {
        let mut kubectl = Command::new(resolve_kubectl_binary()?);
        let has_kubeconfig = command
            .iter()
            .skip(1)
            .any(|arg| arg == "--kubeconfig" || arg.starts_with("--kubeconfig="));
        if !has_kubeconfig {
            kubectl.arg("--kubeconfig").arg(&kubeconfig);
        }
        kubectl.args(&command[1..]);
        kubectl
    } else {
        let mut other = Command::new(program);
        other.args(&command[1..]);
        other
    };

    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_clean_env(&mut cmd, &kubeconfig);

    let status = cmd.status()?;
    Ok(status.code().unwrap_or(1))
}
