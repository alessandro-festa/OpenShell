# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""OpenShell - Agent execution and management SDK."""

from __future__ import annotations

import contextlib

# Proto stubs may not be generated yet (requires Rust build).
# Suppress so subpackages like openshell.prover can still be imported.
with contextlib.suppress(ImportError):
    from .sandbox import (  # noqa: F401 — intentional re-exports
        ClusterInferenceConfig,
        ExecChunk,
        ExecResult,
        InferenceRouteClient,
        Sandbox,
        SandboxClient,
        SandboxError,
        SandboxRef,
        SandboxSession,
        TlsConfig,
    )

try:
    from importlib.metadata import version

    __version__ = version("openshell")
except Exception:
    __version__ = "0.0.0"

_SDK_SYMBOLS = [
    "ClusterInferenceConfig",
    "ExecChunk",
    "ExecResult",
    "InferenceRouteClient",
    "Sandbox",
    "SandboxClient",
    "SandboxError",
    "SandboxRef",
    "SandboxSession",
    "TlsConfig",
]

__all__ = [s for s in _SDK_SYMBOLS if s in dir()] + ["__version__"]
