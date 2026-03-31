# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""Tests for the OpenShell Policy Prover."""

from pathlib import Path

from openshell.prover.binary_registry import load_binary_registry
from openshell.prover.credential_loader import load_credential_set
from openshell.prover.policy_parser import FilesystemPolicy, PolicyIntent, parse_policy
from openshell.prover.queries import RiskLevel, run_all_queries
from openshell.prover.z3_model import build_model

_TESTDATA = Path(__file__).parent / "testdata"
_REGISTRY = Path(__file__).parent / "registry"


def _run_prover(policy_name: str, credentials_name: str):
    policy = parse_policy(_TESTDATA / policy_name)
    creds = load_credential_set(_TESTDATA / credentials_name, _REGISTRY)
    binaries = load_binary_registry(_REGISTRY)
    model = build_model(policy, creds, binaries)
    return run_all_queries(model)


def test_parse_policy():
    """Verify policy parsing produces correct structure."""
    policy = parse_policy(_TESTDATA / "policy.yaml")
    assert policy.version == 1
    assert "github_readonly" in policy.network_policies

    rule = policy.network_policies["github_readonly"]
    assert len(rule.endpoints) == 2
    assert len(rule.binaries) == 3

    ep0 = rule.endpoints[0]
    assert ep0.host == "api.github.com"
    assert ep0.is_l7_enforced
    assert ep0.intent == PolicyIntent.READ_ONLY

    ep1 = rule.endpoints[1]
    assert ep1.host == "github.com"
    assert not ep1.is_l7_enforced
    assert ep1.intent == PolicyIntent.L4_ONLY


def test_filesystem_policy():
    """Verify filesystem policy parsing including workdir."""
    policy = parse_policy(_TESTDATA / "policy.yaml")
    fs = policy.filesystem_policy
    assert "/usr" in fs.read_only
    assert "/sandbox" in fs.read_write
    assert "/sandbox" in fs.readable_paths
    assert "/usr" in fs.readable_paths


def test_include_workdir_default():
    """Workdir is included in readable_paths by default."""
    fs = FilesystemPolicy(read_only=["/usr"])
    assert "/sandbox" in fs.readable_paths
    assert "/usr" in fs.readable_paths


def test_include_workdir_false():
    """Workdir excluded when include_workdir is False."""
    fs = FilesystemPolicy(include_workdir=False, read_only=["/usr"])
    assert "/sandbox" not in fs.readable_paths
    assert "/usr" in fs.readable_paths


def test_include_workdir_no_duplicate():
    """Workdir not duplicated if already in read_write."""
    fs = FilesystemPolicy(read_write=["/sandbox"])
    assert fs.readable_paths.count("/sandbox") == 1


def test_git_push_bypass_findings():
    """End-to-end: detect git push bypass in L4-only + L7 policy."""
    findings = _run_prover("policy.yaml", "credentials.yaml")
    risks = {f.risk for f in findings}
    queries = {f.query for f in findings}

    assert RiskLevel.CRITICAL in risks, "Should detect critical exfil paths"
    assert RiskLevel.HIGH in risks, "Should detect write bypass"
    assert "data_exfiltration" in queries
    assert "write_bypass" in queries

    # Only 2 query types in v1
    assert len(queries) == 2

    # Verify git bypass specifically detected
    write_findings = [f for f in findings if f.query == "write_bypass"]
    assert len(write_findings) == 1
    bypass_binaries = {p.binary for p in write_findings[0].paths}
    assert "/usr/bin/git" in bypass_binaries, "Git should be flagged for L7 bypass"


def test_empty_policy_no_findings():
    """Deny-all policy should produce no findings."""
    findings = _run_prover("empty_policy.yaml", "empty_credentials.yaml")
    assert len(findings) == 0
