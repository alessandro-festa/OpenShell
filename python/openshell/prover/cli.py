# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""CLI entry point for OpenShell Policy Prover (OPP)."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from rich.console import Console

from .binary_registry import load_binary_registry
from .credential_loader import load_credential_set
from .policy_parser import parse_policy
from .queries import run_all_queries
from .report import render_compact, render_report
from .z3_model import build_model

_DEFAULT_REGISTRY = Path(__file__).parent / "registry"


def main():
    parser = argparse.ArgumentParser(
        prog="opp",
        description="OpenShell Policy Prover — formal verification for sandbox policies",
    )
    subparsers = parser.add_subparsers(dest="command")

    verify_parser = subparsers.add_parser(
        "prove",
        help="Prove properties of a sandbox policy — or find counterexamples",
    )
    verify_parser.add_argument(
        "--policy",
        required=True,
        type=Path,
        help="Path to OpenShell sandbox policy YAML",
    )
    verify_parser.add_argument(
        "--credentials",
        required=True,
        type=Path,
        help="Path to credential descriptor YAML",
    )
    verify_parser.add_argument(
        "--registry",
        type=Path,
        default=_DEFAULT_REGISTRY,
        help="Path to capability registry directory (default: bundled registry)",
    )
    verify_parser.add_argument(
        "--accepted-risks",
        type=Path,
        default=None,
        help="Path to accepted risks YAML (findings matching these are marked accepted)",
    )
    verify_parser.add_argument(
        "--compact",
        action="store_true",
        help="One-line-per-finding output (for CI)",
    )

    args = parser.parse_args()

    if args.command is None:
        parser.print_help()
        sys.exit(1)

    if args.command == "prove":
        exit_code = cmd_prove(args)
        sys.exit(exit_code)


def cmd_prove(args) -> int:
    """Execute the prove command."""
    console = Console()

    try:
        policy = parse_policy(args.policy)
    except Exception as e:
        console.print(f"[red]Error loading policy: {e}[/red]")
        return 2

    try:
        credential_set = load_credential_set(args.credentials, args.registry)
    except Exception as e:
        console.print(f"[red]Error loading credentials: {e}[/red]")
        return 2

    try:
        binary_registry = load_binary_registry(args.registry)
    except Exception as e:
        console.print(f"[red]Error loading binary registry: {e}[/red]")
        return 2

    model = build_model(policy, credential_set, binary_registry)
    findings = run_all_queries(model)

    if args.accepted_risks:
        try:
            from .accepted_risks import apply_accepted_risks, load_accepted_risks

            accepted = load_accepted_risks(args.accepted_risks)
            findings = apply_accepted_risks(findings, accepted)
        except Exception as e:
            console.print(f"[red]Error loading accepted risks: {e}[/red]")
            return 2

    if args.compact:
        return render_compact(
            findings,
            str(args.policy),
            str(args.credentials),
            console=console,
        )
    else:
        return render_report(
            findings,
            str(args.policy),
            str(args.credentials),
            console=console,
        )


if __name__ == "__main__":
    main()
