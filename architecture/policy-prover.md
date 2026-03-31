# OpenShell Policy Prover (OPP)

Formal verification for AI agent sandbox policies.

OPP answers the question that no other agent security tool can: *"What can this agent actually do?"* — not what the policy says, but what is mathematically reachable given the full interaction of policy rules, credential scopes, binary capabilities, and enforcement layers.

Every policy has properties. OPP proves them — or shows you exactly how they break.

## The Problem

Agent sandboxes enforce policies. Policies express intent: "read-only GitHub access," "no external network," "inference only." But intent and enforcement are not the same thing.

Consider a common OpenShell policy: read-only access to the GitHub API, enforced at L7 with HTTP method restrictions. The policy blocks `POST`, `PUT`, `DELETE` on the REST API. Looks secure.

But the sandbox also allows `git` to connect to `github.com` on port 443. Git doesn't use the REST API — it speaks its own wire protocol inside the HTTPS tunnel. The L7 rules never fire. The agent's GitHub token has `repo` scope. **Git push succeeds. The "read-only" policy didn't prevent a write.**

This isn't a bug in the sandbox. Each layer works correctly in isolation. The problem is that security properties emerge from the *interaction* of layers — and no one is checking that interaction today.

## The Approach

OPP uses **formal methods** — the same class of techniques AWS uses to verify S3 bucket policies and IAM permissions — to reason about the complete enforcement model.

The tool takes three inputs:

1. **The sandbox policy** (OpenShell YAML) — what the proxy enforces
2. **Credential descriptors** — what the injected tokens can actually authorize
3. **Binary capability registry** — what each binary in the sandbox can do

It encodes these as constraints in a **Z3 SMT solver** and asks reachability questions. The solver doesn't guess or estimate — it either proves a property holds or produces a concrete counterexample showing exactly how it can be violated.

### Formal Verification vs. Other Approaches

| Approach | Can it find gaps? | Guarantees? | Scales? |
|----------|------------------|-------------|---------|
| **Manual review** | Sometimes | None — depends on reviewer expertise | No |
| **LLM classifier** | Often | None — probabilistic, can be bypassed | Yes |
| **Penetration testing** | If the tester thinks of it | None — absence of evidence isn't evidence of absence | No |
| **Formal verification (OPP)** | **Always, if the model covers it** | **Mathematical proof** — if it says safe, it's safe | **Yes — solver handles combinatorial explosion** |

The key distinction: an LLM classifier says "this action *looks* safe." OPP says "there is *no possible path* to this action" — or produces the exact path if one exists.

This is not a replacement for runtime enforcement (the proxy, OPA engine, Landlock, seccomp). It's a **design-time verification layer** that catches gaps before the agent runs. Defense in depth: enforce at runtime, prove at authoring time.

## What It Detects

### Data Exfiltration Paths

*"Can data leave the sandbox?"*

OPP maps every path from readable filesystem locations to writable egress channels: L4-only endpoints where binaries can POST freely, git push channels, raw TCP via netcat, DNS exfiltration, even inference relay side channels. If a path exists, the solver finds it.

**Example finding:**
```
CRITICAL  Data exfiltration possible
          L4-only: github.com:443; wire protocol bypass: api.github.com:443
```

### Write Bypass Detection

*"Can the agent modify something despite a read-only policy?"*

OPP checks whether any combination of binary capabilities, credential scopes, and transport protocols allows write operations that the policy intends to block.

**Example finding:**
```
HIGH  Write bypass — read-only intent violated
      L4-only + wire protocol: api.github.com:443, github.com:443
```

### Overpermissive HTTP Methods

*"Does this policy allow destructive operations the agent doesn't need?"*

OPP flags `method: "*"` wildcards and unnecessary `DELETE` access on endpoints that share hosts with management APIs. An inference endpoint that only needs `POST /v1/chat/completions` shouldn't also allow `DELETE /v2/nvcf/assets/{id}`.

### L4 Policy Gaps

*"Are there endpoints with no HTTP inspection at all?"*

OPP flags endpoints missing `protocol: rest` where HTTP-capable binaries have access — meaning all traffic passes uninspected. Especially flagged when sibling endpoints in the same policy group DO have L7 enforcement, indicating inconsistent configuration.

### Binary Inheritance Audit

*"Which processes inherit network access they weren't explicitly granted?"*

OpenShell's binary matching includes ancestor chain matching — if `claude` is allowed, any process `claude` spawns inherits that access. OPP traces the full inheritance tree and flags binaries with capabilities that exceed the policy's apparent intent.

### Inference Relay Risk

*"Can the agent reach external resources through inference.local?"*

The `inference.local` virtual host always bypasses OPA policy evaluation. If the backing model supports tool use or function calling, the agent may be able to instruct it to access external services — creating a policy-free side channel.

## How It Works

```
Policy YAML ──┐
               │    ┌─────────────────┐    ┌───────────┐
Credentials ───┼───>│  Z3 Constraint  │───>│  SAT?     │──> Counterexample path
               │    │  Encoder        │    │  UNSAT?   │──> Proven safe
Binaries ──────┘    └─────────────────┘    └───────────┘
```

1. **Parse** the policy YAML into an enforcement model: which endpoints are L4-only vs L7-enforced, what methods/paths are allowed, which binaries have access.

2. **Load** credential descriptors that map token scopes to concrete API capabilities (e.g., GitHub PAT `repo` scope -> can push, create issues, delete repos).

3. **Load** binary capability descriptors that describe what each binary can do: protocols it speaks, whether it bypasses L7, what child processes it spawns, whether it can exfiltrate data.

4. **Encode** all of the above as Z3 SMT constraints — boolean variables and logical implications representing every possible action path.

5. **Query** the solver: "Is data exfiltration possible?" "Can the agent write despite read-only intent?" "Which binaries inherit unintended access?"

6. **Report** findings with concrete paths, risk ratings, and remediation suggestions.

### The Knowledge Base

OPP's accuracy depends on the quality of its capability descriptors — how well it models what each API, credential type, and binary can do.

**Trusted sources only.** OPP restricts its knowledge base to:

- **Published API specs** (OpenAPI/Swagger) for well-known services
- **Curated binary descriptors** for the binaries that ship in the base sandbox image (~17 binaries with well-known, stable behavior)
- **Credential scope documentation** from the credential provider (e.g., GitHub's PAT scope definitions)

Anything not in the knowledge base is flagged conservatively — OPP assumes unknown binaries can exfiltrate and construct HTTP. This is deliberate: a prover that claims more than it can prove is worse than no prover at all.

The knowledge base is extensible. Registry YAML files live at `python/openshell/prover/registry/`.

## Usage

### CLI

```bash
# Via the Rust CLI (recommended)
openshell policy prove \
  --policy policy.yaml \
  --credentials credentials.yaml

# Compact output for CI
openshell policy prove \
  --policy policy.yaml \
  --credentials credentials.yaml \
  --compact

# HTML report with interactive Mermaid diagrams
openshell policy prove \
  --policy policy.yaml \
  --credentials credentials.yaml \
  --html report.html

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

**Exit codes:** 0 = PASS (advisories only), 1 = FAIL (critical/high gaps), 2 = input error.

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

### Source Layout

```
python/openshell/prover/
    cli.py                 # CLI entry point (argparse)
    policy_parser.py       # OpenShell YAML -> PolicyModel
    z3_model.py            # PolicyModel + credentials + binaries -> Z3 constraints
    queries.py             # Verification queries (6 query types)
    binary_registry.py     # Binary capability descriptors
    credential_loader.py   # Credential + API capability descriptors
    report.py              # Terminal, compact, Mermaid, and HTML output
    accepted_risks.py      # Risk acceptance filtering
    registry/              # YAML capability descriptors (binaries, APIs, credentials)
```

### Rust CLI Integration

The `openshell policy prove` subcommand in `crates/openshell-cli/src/main.rs` delegates to the Python prover via `uv run python3 -m openshell.prover.cli`. This keeps the prover logic in Python (where Z3 bindings are most mature) while providing a unified CLI experience.

### Dependencies

The prover is an optional dependency group to avoid bloating the base SDK:

```toml
[project.optional-dependencies]
prover = ["z3-solver>=4.12", "pyyaml>=6.0", "rich>=13.0"]
```

Install with: `uv pip install '.[prover]'` or include in dev dependencies via `uv sync`.

## Differentiation

### vs. OPA / Rego

OPA evaluates policies at runtime on a per-request basis using Rego rules. OPP operates at design time and reasons about the *complete enforcement model* — all possible request paths, not just the one being evaluated now.

### vs. Static Policy Linting

Tools like `opa check` or conftest validate that a policy is syntactically correct and internally consistent. They don't model what the policy *allows in practice* given credentials, binary capabilities, and layer interactions.

### vs. Penetration Testing

A pentest might find the git push bypass — if the tester thinks to try it. OPP finds it automatically, systematically, and exhaustively (within its model). And it runs in seconds, not days.

### vs. LLM Classifiers

An LLM classifier says "this action *looks* safe." OPP says "there is *no possible path* to this action." The two are complementary — classifiers catch runtime misbehavior, OPP catches policy design flaws.

| | LLM Classifier | Policy Prover |
|---|---|---|
| **When** | Runtime (per-action) | Design time (per-policy) |
| **Method** | LLM classification | SMT formal verification |
| **Guarantee** | Probabilistic | Mathematical proof |
| **Catches** | Agent deviating from intent | Policy gaps across layers |
| **Vulnerable to** | Prompt injection, false negatives | Incomplete capability model |
