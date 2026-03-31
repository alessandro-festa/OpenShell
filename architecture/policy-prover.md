# OpenShell Policy Prover (OPP)

Formal verification for AI agent sandbox policies.

OPP answers two yes/no security questions about any sandbox policy:

1. **"Can data leave this sandbox?"** — exfiltration analysis
2. **"Can the agent write despite read-only intent?"** — write bypass detection

Any finding = FAIL. No findings = PASS. Like AWS Zelkova for S3 bucket policies, but for agent sandboxes.

## The Problem

Agent sandboxes enforce policies. Policies express intent: "read-only GitHub access," "no external network," "inference only." But intent and enforcement are not the same thing.

Consider a common OpenShell policy: read-only access to the GitHub API, enforced at L7 with HTTP method restrictions. The policy blocks `POST`, `PUT`, `DELETE` on the REST API. Looks secure.

But the sandbox also allows `git` to connect to `github.com` on port 443. Git doesn't use the REST API — it speaks its own wire protocol inside the HTTPS tunnel. The L7 rules never fire. The agent's GitHub token has `repo` scope. **Git push succeeds. The "read-only" policy didn't prevent a write.**

This isn't a bug in the sandbox. Each layer works correctly in isolation. The problem is that security properties emerge from the *interaction* of layers — and no one is checking that interaction today.

## The Approach

OPP uses **Z3 SMT solving** — the same class of formal methods AWS uses to verify S3 bucket policies — to reason about the complete enforcement model.

Three inputs:

1. **The sandbox policy** (OpenShell YAML) — what the proxy enforces
2. **Credential descriptors** — what the injected tokens can actually authorize
3. **Binary capability registry** — what each binary in the sandbox can do

It encodes these as Z3 constraints and asks reachability questions. The solver either proves a property holds or produces a concrete counterexample showing exactly how it can be violated.

## What It Detects

### Data Exfiltration Paths

*"Can data leave this sandbox?"*

Maps every path from readable filesystem locations (including the workdir) to writable egress channels: L4-only endpoints where binaries can POST freely, git push channels, raw TCP via netcat. If a path exists, the solver finds it.

### Write Bypass Detection

*"Can the agent write despite read-only intent?"*

Checks whether any combination of binary capabilities, credential scopes, and transport protocols allows write operations that the policy intends to block.

## How It Works

```
Policy YAML ──┐
               │    ┌─────────────────┐    ┌───────────┐
Credentials ───┼───>│  Z3 Constraint  │───>│  SAT?     │──> Counterexample path
               │    │  Encoder        │    │  UNSAT?   │──> Proven safe
Binaries ──────┘    └─────────────────┘    └───────────┘
```

1. **Parse** the policy YAML into an enforcement model
2. **Load** credential descriptors mapping token scopes to API capabilities
3. **Load** binary capability descriptors (protocols, spawn chains, exfil ability)
4. **Encode** as Z3 SMT constraints
5. **Query** the solver: exfiltration possible? Write bypass possible?
6. **Report** findings with concrete paths and remediation

### Accepted Risks

Teams can acknowledge known paths via an accepted-risks YAML file. For example: "we accept that api.anthropic.com is an exfil path because that's our inference provider." Accepted findings show as PASS with a note, not FAIL.

## Usage

### CLI

```bash
# Via the Rust CLI
openshell policy prove \
  --policy policy.yaml \
  --credentials credentials.yaml

# Compact output for CI
openshell policy prove \
  --policy policy.yaml \
  --credentials credentials.yaml \
  --compact

# With accepted risks
openshell policy prove \
  --policy policy.yaml \
  --credentials credentials.yaml \
  --accepted-risks accepted.yaml

# Direct Python invocation
python3 -m openshell.prover.cli prove \
  --policy policy.yaml \
  --credentials credentials.yaml
```

**Exit codes:** 0 = PASS, 1 = FAIL (exfiltration or write bypass found), 2 = input error.

### Python API

```python
from openshell.prover.policy_parser import parse_policy
from openshell.prover.credential_loader import load_credential_set
from openshell.prover.binary_registry import load_binary_registry
from openshell.prover.z3_model import build_model
from openshell.prover.queries import run_all_queries

policy = parse_policy("policy.yaml")
creds = load_credential_set("credentials.yaml", "registry/")
binaries = load_binary_registry("registry/")
model = build_model(policy, creds, binaries)
findings = run_all_queries(model)

for f in findings:
    print(f"{f.risk.value}: {f.title}")
```

### Agent Skill

The `harden-sandbox-policy` skill wraps OPP for iterative policy hardening:

1. `generate-sandbox-policy` — author a policy from requirements
2. `harden-sandbox-policy` — verify and tighten the authored policy
3. Iterate until the prover returns PASS

## Architecture

```
python/openshell/prover/
    cli.py                 # CLI entry point
    policy_parser.py       # OpenShell YAML -> PolicyModel
    z3_model.py            # Z3 constraint encoding
    queries.py             # 2 verification queries
    binary_registry.py     # Binary capability descriptors
    credential_loader.py   # Credential + API capability descriptors
    report.py              # Terminal and compact output
    accepted_risks.py      # Risk acceptance filtering
    registry/              # YAML capability descriptors
```

The `openshell policy prove` Rust CLI subcommand delegates to the Python prover via `uv run python3 -m openshell.prover.cli`.

### Dependencies

Optional dependency group — does not bloat the base SDK:

```toml
[project.optional-dependencies]
prover = ["z3-solver>=4.12", "pyyaml>=6.0", "rich>=13.0"]
```
