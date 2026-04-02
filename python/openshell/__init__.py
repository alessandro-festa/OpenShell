# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

"""OpenShell - Agent execution and management SDK."""

from __future__ import annotations

import logging
import warnings

_log = logging.getLogger(__name__)

try:
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
except ImportError as _err:
    _msg = str(_err)
    if "proto" in _msg or "grpc" in _msg or "_pb2" in _msg:
        _log.debug("SDK symbols unavailable (proto stubs not generated): %s", _err)
    else:
        warnings.warn(
            f"openshell SDK symbols could not be imported: {_err}",
            ImportWarning,
            stacklevel=2,
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
