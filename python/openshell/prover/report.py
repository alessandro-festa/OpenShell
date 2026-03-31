# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""Terminal report and compact CI output for verification findings."""

from __future__ import annotations

from pathlib import Path

from rich.console import Console
from rich.panel import Panel
from rich.table import Table
from rich.text import Text
from rich.tree import Tree

from .queries import (
    ExfilPath,
    Finding,
    RiskLevel,
    WriteBypassPath,
)

RISK_COLORS = {
    RiskLevel.CRITICAL: "bold red",
    RiskLevel.HIGH: "red",
}

RISK_ICONS = {
    RiskLevel.CRITICAL: "CRITICAL",
    RiskLevel.HIGH: "HIGH",
}


def _hl(text: str) -> str:
    """Highlight key terms in finding summaries with rich markup."""
    highlights = {
        "L4-only": "[bold red]L4-only[/bold red]",
        "L7": "[bold green]L7[/bold green]",
        "wire protocol": "[bold red]wire protocol[/bold red]",
        "no HTTP inspection": "[bold red]no HTTP inspection[/bold red]",
    }
    for term, markup in highlights.items():
        text = text.replace(term, markup)
    return text


def _compact_detail(finding: Finding) -> str:
    """Generate a short detail string for compact output."""
    if finding.query == "data_exfiltration":
        by_status: dict[str, set[str]] = {}
        for p in finding.paths:
            if hasattr(p, "l7_status") and hasattr(p, "endpoint_host"):
                by_status.setdefault(p.l7_status, set()).add(
                    f"{p.endpoint_host}:{p.endpoint_port}"
                )
        parts = []
        if "l4_only" in by_status:
            parts.append(f"L4-only: {', '.join(sorted(by_status['l4_only']))}")
        if "l7_bypassed" in by_status:
            parts.append(
                f"wire protocol bypass: {', '.join(sorted(by_status['l7_bypassed']))}"
            )
        if "l7_allows_write" in by_status:
            parts.append(f"L7 write: {', '.join(sorted(by_status['l7_allows_write']))}")
        return "; ".join(parts)
    elif finding.query == "write_bypass":
        reasons = set()
        endpoints = set()
        for p in finding.paths:
            if hasattr(p, "bypass_reason"):
                reasons.add(p.bypass_reason)
            if hasattr(p, "endpoint_host"):
                endpoints.add(f"{p.endpoint_host}:{p.endpoint_port}")
        ep_list = ", ".join(sorted(endpoints))
        if "l4_only" in reasons and "l7_bypass_protocol" in reasons:
            return f"L4-only + wire protocol: {ep_list}"
        if "l4_only" in reasons:
            return f"L4-only (no inspection): {ep_list}"
        if "l7_bypass_protocol" in reasons:
            return f"wire protocol bypasses L7: {ep_list}"
    return ""


def render_compact(
    findings: list[Finding],
    _policy_path: str,
    _credentials_path: str,
    console: Console | None = None,
) -> int:
    """Compact output for CI. Two lines per finding: summary + detail."""
    if console is None or console.width > 72:
        console = Console(width=72)

    active = [f for f in findings if not f.accepted]
    accepted_findings = [f for f in findings if f.accepted]

    compact_titles = {
        "data_exfiltration": "Data exfiltration possible",
        "write_bypass": "Write bypass — read-only intent violated",
    }

    for finding in active:
        style = RISK_COLORS[finding.risk]
        label = RISK_ICONS[finding.risk]
        title = compact_titles.get(finding.query, finding.title)
        detail = _compact_detail(finding)

        console.print(f"  [{style}]{label:>8}[/{style}]  {_hl(title)}")
        if detail:
            console.print(f"             {_hl(detail)}")
        console.print()

    for finding in accepted_findings:
        title = compact_titles.get(finding.query, finding.title)
        console.print(f"  [dim]ACCEPTED  {title}[/dim]")

    if accepted_findings:
        console.print()

    counts = {}
    for f in active:
        counts[f.risk] = counts.get(f.risk, 0) + 1
    has_critical = RiskLevel.CRITICAL in counts
    has_high = RiskLevel.HIGH in counts
    accepted_note = f", {len(accepted_findings)} accepted" if accepted_findings else ""

    if has_critical or has_high:
        n = counts.get(RiskLevel.CRITICAL, 0) + counts.get(RiskLevel.HIGH, 0)
        console.print(
            f"  [bold white on red] FAIL [/bold white on red] {n} critical/high gaps{accepted_note}"
        )
        return 1
    elif accepted_findings:
        console.print(
            f"  [bold white on green] PASS [/bold white on green] all findings accepted{accepted_note}"
        )
        return 0
    else:
        console.print("  [bold white on green] PASS [/bold white on green] no findings")
        return 0


def render_report(
    findings: list[Finding],
    policy_path: str,
    credentials_path: str,
    console: Console | None = None,
) -> int:
    """Render findings to the terminal. Returns exit code (0 = pass, 1 = fail)."""
    if console is None or console.width > 80:
        console = Console(width=80)

    policy_name = Path(policy_path).name
    creds_name = Path(credentials_path).name

    console.print()
    console.print(
        Panel(
            "[bold]OpenShell Policy Prover[/bold]",
            border_style="blue",
        )
    )
    console.print(f"  Policy:      {policy_name}")
    console.print(f"  Credentials: {creds_name}")
    console.print()

    active = [f for f in findings if not f.accepted]
    accepted_findings = [f for f in findings if f.accepted]

    counts = {}
    for f in active:
        counts[f.risk] = counts.get(f.risk, 0) + 1

    summary = Table(title="Finding Summary", show_header=True, border_style="dim")
    summary.add_column("Risk Level", style="bold")
    summary.add_column("Count", justify="right")
    for level in [RiskLevel.CRITICAL, RiskLevel.HIGH]:
        if level in counts:
            style = RISK_COLORS[level]
            summary.add_row(
                Text(RISK_ICONS[level], style=style),
                Text(str(counts[level]), style=style),
            )
    if accepted_findings:
        summary.add_row(
            Text("ACCEPTED", style="dim"),
            Text(str(len(accepted_findings)), style="dim"),
        )
    console.print(summary)
    console.print()

    if not active and not accepted_findings:
        console.print("[bold green]No findings. Policy posture is clean.[/bold green]")
        return 0

    for i, finding in enumerate(active, 1):
        risk_style = RISK_COLORS[finding.risk]
        risk_label = RISK_ICONS[finding.risk]

        console.print(
            Panel(
                f"[{risk_style}]{risk_label}[/{risk_style}]  {finding.title}",
                border_style=risk_style,
                title=f"Finding #{i}",
                title_align="left",
            )
        )
        console.print(f"  {finding.description}")
        console.print()

        if finding.paths:
            _render_paths(console, finding.paths)

        if finding.remediation:
            console.print("  [bold]Remediation:[/bold]")
            for r in finding.remediation:
                console.print(f"    - {r}")
            console.print()

    if accepted_findings:
        console.print(Panel("[dim]Accepted Risks[/dim]", border_style="dim"))
        for finding in accepted_findings:
            console.print(f"  [dim]{RISK_ICONS[finding.risk]}  {finding.title}[/dim]")
            console.print(f"  [dim]Reason: {finding.accepted_reason}[/dim]")
            console.print()

    has_critical = RiskLevel.CRITICAL in counts
    has_high = RiskLevel.HIGH in counts
    accepted_note = f" ({len(accepted_findings)} accepted)" if accepted_findings else ""

    if has_critical or has_high:
        console.print(
            Panel(
                f"[bold red]FAIL[/bold red] — Critical/high gaps found.{accepted_note}",
                border_style="red",
            )
        )
        return 1
    elif accepted_findings:
        console.print(
            Panel(
                f"[bold green]PASS[/bold green] — All findings accepted.{accepted_note}",
                border_style="green",
            )
        )
        return 0
    else:
        console.print(
            Panel(
                "[bold green]PASS[/bold green] — No findings.",
                border_style="green",
            )
        )
        return 0


def _render_paths(console: Console, paths: list) -> None:
    """Render finding paths as a table or tree depending on type."""
    if not paths:
        return

    first = paths[0]
    if isinstance(first, ExfilPath):
        _render_exfil_paths(console, paths)
    elif isinstance(first, WriteBypassPath):
        _render_write_bypass_paths(console, paths)


def _render_exfil_paths(console: Console, paths: list[ExfilPath]) -> None:
    table = Table(show_header=True, border_style="dim", padding=(0, 1))
    table.add_column("Binary", style="bold")
    table.add_column("Endpoint")
    table.add_column("L7 Status")
    table.add_column("Mechanism", max_width=60)

    for p in paths:
        l7_style = {
            "l4_only": "bold red",
            "l7_bypassed": "red",
            "l7_allows_write": "yellow",
        }.get(p.l7_status, "white")

        table.add_row(
            p.binary,
            f"{p.endpoint_host}:{p.endpoint_port}",
            Text(p.l7_status, style=l7_style),
            p.mechanism,
        )

    console.print(table)
    console.print()


def _render_write_bypass_paths(console: Console, paths: list[WriteBypassPath]) -> None:
    for p in paths:
        tree = Tree(f"[bold]{p.binary}[/bold] -> {p.endpoint_host}:{p.endpoint_port}")
        tree.add(f"Policy: {p.policy_name} (intent: {p.policy_intent})")
        tree.add(f"[red]Bypass: {p.bypass_reason}[/red]")
        if p.credential_actions:
            cred_branch = tree.add("Credential enables:")
            for action in p.credential_actions[:5]:
                cred_branch.add(action)
            if len(p.credential_actions) > 5:
                cred_branch.add(f"... and {len(p.credential_actions) - 5} more")
        console.print(tree)

    console.print()
