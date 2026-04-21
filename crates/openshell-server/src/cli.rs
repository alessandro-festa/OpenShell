// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared CLI entrypoint for the gateway binaries.

use clap::{ArgAction, Args, Command, CommandFactory, FromArgMatches, Parser};
use miette::{IntoDiagnostic, Result};
use openshell_core::ComputeDriverKind;
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::compute::VmComputeConfig;
use crate::{run_server, tracing_bus::TracingLogBus};

/// TLS for the main gRPC/HTTP listener (gateway server identity and mTLS client CA).
#[derive(Args, Debug)]
#[command(next_help_heading = "Server TLS")]
struct ServerTlsArgs {
    /// Path to PEM certificate file (required unless server TLS is disabled).
    #[arg(
        long = "server.tls.cert",
        visible_alias = "tls-cert",
        env = "OPENSHELL_TLS_CERT"
    )]
    cert: Option<PathBuf>,

    /// Path to PEM private key file (required unless server TLS is disabled).
    #[arg(long = "server.tls.key", visible_alias = "tls-key", env = "OPENSHELL_TLS_KEY")]
    key: Option<PathBuf>,

    /// Path to PEM CA certificate for mTLS client verification.
    #[arg(
        long = "server.tls.client-ca",
        visible_alias = "tls-client-ca",
        env = "OPENSHELL_TLS_CLIENT_CA"
    )]
    client_ca: Option<PathBuf>,

    /// Listen on plaintext HTTP — no gateway TLS (e.g. behind a reverse proxy or tunnel).
    #[arg(
        long = "server.tls.disable",
        visible_alias = "disable-tls",
        env = "OPENSHELL_DISABLE_TLS",
        default_value_t = false,
        action = ArgAction::SetTrue
    )]
    disable: bool,

    /// Accept TLS connections without a client certificate (application-layer auth only).
    #[arg(
        long = "server.tls.disable-gateway-auth",
        visible_alias = "disable-gateway-auth",
        env = "OPENSHELL_DISABLE_GATEWAY_AUTH",
        default_value_t = false,
        action = ArgAction::SetTrue
    )]
    disable_gateway_auth: bool,
}

/// Main listener: TCP port (`0.0.0.0:<port>`) and gateway TLS.
#[derive(Args, Debug)]
#[command(next_help_heading = "Server")]
struct ServerArgs {
    /// TCP listen port (all interfaces).
    #[arg(
        long = "server.port",
        visible_alias = "port",
        default_value_t = openshell_core::DEFAULT_SERVER_PORT,
        env = "OPENSHELL_SERVER_PORT"
    )]
    port: u16,

    #[command(flatten)]
    tls: ServerTlsArgs,
}

/// Health listener options (plaintext HTTP for `/health`, `/healthz`, `/readyz`).
#[derive(Args, Debug)]
#[command(next_help_heading = "Health")]
struct HealthArgs {
    /// Plaintext port (all interfaces). Binds `0.0.0.0:<port>`.
    #[arg(
        long = "health.port",
        visible_alias = "health-http-port",
        default_value_t = openshell_core::DEFAULT_HEALTH_SERVER_PORT,
        env = "OPENSHELL_HEALTH_HTTP_PORT"
    )]
    port: u16,
}

/// `OpenShell` gateway process - gRPC and HTTP server with protocol multiplexing.
#[derive(Parser, Debug)]
#[command(version = openshell_core::VERSION)]
#[command(about = "OpenShell gRPC/HTTP server", long_about = None)]
struct GatewayArgs {
    #[command(flatten)]
    server: ServerArgs,

    #[command(flatten)]
    health: HealthArgs,

    /// Log level (trace, debug, info, warn, error).
    #[arg(long, default_value = "info", env = "OPENSHELL_LOG_LEVEL")]
    log_level: String,

    /// Database URL for persistence.
    #[arg(long, env = "OPENSHELL_DB_URL", required = true)]
    db_url: String,

    /// Compute drivers configured for this gateway.
    ///
    /// Accepts a comma-delimited list such as `kubernetes` or
    /// `kubernetes,podman`. The configuration format is future-proofed for
    /// multiple drivers, but the gateway currently requires exactly one.
    #[arg(
        long,
        alias = "driver",
        env = "OPENSHELL_DRIVERS",
        value_delimiter = ',',
        default_value = "kubernetes",
        value_parser = parse_compute_driver
    )]
    drivers: Vec<ComputeDriverKind>,

    /// Kubernetes namespace for sandboxes.
    #[arg(long, env = "OPENSHELL_SANDBOX_NAMESPACE", default_value = "default")]
    sandbox_namespace: String,

    /// Default container image for sandboxes.
    #[arg(long, env = "OPENSHELL_SANDBOX_IMAGE")]
    sandbox_image: Option<String>,

    /// Kubernetes imagePullPolicy for sandbox pods (Always, IfNotPresent, Never).
    #[arg(long, env = "OPENSHELL_SANDBOX_IMAGE_PULL_POLICY")]
    sandbox_image_pull_policy: Option<String>,

    /// gRPC endpoint for sandboxes to callback to `OpenShell`.
    /// This should be reachable from within the Kubernetes cluster.
    #[arg(long, env = "OPENSHELL_GRPC_ENDPOINT")]
    grpc_endpoint: Option<String>,

    /// Public host for the SSH gateway.
    #[arg(long, env = "OPENSHELL_SSH_GATEWAY_HOST", default_value = "127.0.0.1")]
    ssh_gateway_host: String,

    /// Public port for the SSH gateway.
    #[arg(
        long,
        env = "OPENSHELL_SSH_GATEWAY_PORT",
        default_value_t = openshell_core::DEFAULT_SERVER_PORT
    )]
    ssh_gateway_port: u16,

    /// HTTP path for SSH CONNECT/upgrade.
    #[arg(
        long,
        env = "OPENSHELL_SSH_CONNECT_PATH",
        default_value = "/connect/ssh"
    )]
    ssh_connect_path: String,

    /// SSH port inside sandbox pods.
    #[arg(long, env = "OPENSHELL_SANDBOX_SSH_PORT", default_value_t = 2222)]
    sandbox_ssh_port: u16,

    /// Shared secret for gateway-to-sandbox SSH handshake.
    #[arg(long, env = "OPENSHELL_SSH_HANDSHAKE_SECRET")]
    ssh_handshake_secret: Option<String>,

    /// Allowed clock skew in seconds for SSH handshake.
    #[arg(long, env = "OPENSHELL_SSH_HANDSHAKE_SKEW_SECS", default_value_t = 300)]
    ssh_handshake_skew_secs: u64,

    /// Kubernetes secret name containing client TLS materials for sandbox pods.
    #[arg(long, env = "OPENSHELL_CLIENT_TLS_SECRET_NAME")]
    client_tls_secret_name: Option<String>,

    /// Host gateway IP for sandbox pod hostAliases.
    /// When set, sandbox pods get hostAliases entries mapping
    /// host.docker.internal and host.openshell.internal to this IP.
    #[arg(long, env = "OPENSHELL_HOST_GATEWAY_IP")]
    host_gateway_ip: Option<String>,

    /// Working directory for VM driver sandbox state.
    #[arg(
        long,
        env = "OPENSHELL_VM_DRIVER_STATE_DIR",
        default_value_os_t = VmComputeConfig::default_state_dir()
    )]
    vm_driver_state_dir: PathBuf,

    /// Directory searched for compute-driver binaries (e.g.
    /// `openshell-driver-vm`) when an explicit binary override isn't
    /// configured. When unset, the gateway searches
    /// `$HOME/.local/libexec/openshell`, `/usr/local/libexec/openshell`,
    /// `/usr/local/libexec`, then a sibling of the gateway binary.
    #[arg(long, env = "OPENSHELL_DRIVER_DIR")]
    driver_dir: Option<PathBuf>,

    /// libkrun log level used by the VM helper.
    #[arg(
        long,
        env = "OPENSHELL_VM_KRUN_LOG_LEVEL",
        default_value_t = VmComputeConfig::default_krun_log_level()
    )]
    vm_krun_log_level: u32,

    /// Default vCPU count for VM sandboxes.
    #[arg(
        long,
        env = "OPENSHELL_VM_DRIVER_VCPUS",
        default_value_t = VmComputeConfig::default_vcpus()
    )]
    vm_vcpus: u8,

    /// Default memory allocation for VM sandboxes, in MiB.
    #[arg(
        long,
        env = "OPENSHELL_VM_DRIVER_MEM_MIB",
        default_value_t = VmComputeConfig::default_mem_mib()
    )]
    vm_mem_mib: u32,

    /// CA certificate installed into VM sandboxes for gateway mTLS.
    #[arg(long, env = "OPENSHELL_VM_TLS_CA")]
    vm_tls_ca: Option<PathBuf>,

    /// Client certificate installed into VM sandboxes for gateway mTLS.
    #[arg(long, env = "OPENSHELL_VM_TLS_CERT")]
    vm_tls_cert: Option<PathBuf>,

    /// Client private key installed into VM sandboxes for gateway mTLS.
    #[arg(long, env = "OPENSHELL_VM_TLS_KEY")]
    vm_tls_key: Option<PathBuf>,

}

pub fn command() -> Command {
    GatewayArgs::command()
        .name("openshell-gateway")
        .bin_name("openshell-gateway")
}

pub async fn run_cli() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|e| miette::miette!("failed to install rustls crypto provider: {e:?}"))?;

    let args = GatewayArgs::from_arg_matches(&command().get_matches()).expect("clap validated args");

    run_from_args(args).await
}

async fn run_from_args(args: GatewayArgs) -> Result<()> {
    let tracing_log_bus = TracingLogBus::new();
    tracing_log_bus.install_subscriber(
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level)),
    );

    let bind = SocketAddr::from(([0, 0, 0, 0], args.server.port));

    let tls = if args.server.tls.disable {
        None
    } else {
        let cert_path = args.server.tls.cert.ok_or_else(|| {
            miette::miette!(
                "--server.tls.cert is required when TLS is enabled (use --server.tls.disable / --disable-tls for plaintext)"
            )
        })?;
        let key_path = args.server.tls.key.ok_or_else(|| {
            miette::miette!(
                "--server.tls.key is required when TLS is enabled (use --server.tls.disable / --disable-tls for plaintext)"
            )
        })?;
        let client_ca_path = args.server.tls.client_ca.ok_or_else(|| {
            miette::miette!(
                "--server.tls.client-ca is required when TLS is enabled (use --server.tls.disable / --disable-tls for plaintext)"
            )
        })?;
        Some(openshell_core::TlsConfig {
            cert_path,
            key_path,
            client_ca_path,
            allow_unauthenticated: args.server.tls.disable_gateway_auth,
        })
    };

    let health_bind = SocketAddr::from(([0, 0, 0, 0], args.health.port));

    let mut config = openshell_core::Config::new(tls)
        .with_bind_address(bind)
        .with_health_bind_address(health_bind)
        .with_log_level(&args.log_level);

    config = config
        .with_database_url(args.db_url)
        .with_compute_drivers(args.drivers)
        .with_sandbox_namespace(args.sandbox_namespace)
        .with_ssh_gateway_host(args.ssh_gateway_host)
        .with_ssh_gateway_port(args.ssh_gateway_port)
        .with_ssh_connect_path(args.ssh_connect_path)
        .with_sandbox_ssh_port(args.sandbox_ssh_port)
        .with_ssh_handshake_skew_secs(args.ssh_handshake_skew_secs);

    if let Some(image) = args.sandbox_image {
        config = config.with_sandbox_image(image);
    }

    if let Some(policy) = args.sandbox_image_pull_policy {
        config = config.with_sandbox_image_pull_policy(policy);
    }

    if let Some(endpoint) = args.grpc_endpoint {
        config = config.with_grpc_endpoint(endpoint);
    }

    if let Some(secret) = args.ssh_handshake_secret {
        config = config.with_ssh_handshake_secret(secret);
    }

    if let Some(name) = args.client_tls_secret_name {
        config = config.with_client_tls_secret_name(name);
    }

    if let Some(ip) = args.host_gateway_ip {
        config = config.with_host_gateway_ip(ip);
    }

    let vm_config = VmComputeConfig {
        state_dir: args.vm_driver_state_dir,
        driver_dir: args.driver_dir,
        krun_log_level: args.vm_krun_log_level,
        vcpus: args.vm_vcpus,
        mem_mib: args.vm_mem_mib,
        guest_tls_ca: args.vm_tls_ca,
        guest_tls_cert: args.vm_tls_cert,
        guest_tls_key: args.vm_tls_key,
    };

    if args.server.tls.disable {
        info!("TLS disabled — listening on plaintext HTTP");
    } else if args.server.tls.disable_gateway_auth {
        info!("Gateway auth disabled — accepting connections without client certificates");
    }

    info!(address = %config.health_bind_address, "Health HTTP listener");

    info!(bind = %config.bind_address, "Starting OpenShell server");

    run_server(config, vm_config, tracing_log_bus)
        .await
        .into_diagnostic()
}

fn parse_compute_driver(value: &str) -> std::result::Result<ComputeDriverKind, String> {
    value.parse()
}

#[cfg(test)]
mod tests {
    use super::command;

    #[test]
    fn command_uses_gateway_binary_name() {
        let mut help = Vec::new();
        command().write_long_help(&mut help).unwrap();
        let help = String::from_utf8(help).unwrap();
        assert!(help.contains("openshell-gateway"));
    }

    #[test]
    fn command_exposes_version() {
        let cmd = command();
        let version = cmd.get_version().unwrap();
        assert_eq!(version.to_string(), openshell_core::VERSION);
    }
}
