#!/usr/bin/env bash
# SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

ROOTFS_DIR="${XDG_DATA_HOME:-${HOME}/.local/share}/openshell/gateway/rootfs"

if [ "${OPENSHELL_VM_FORCE_ROOTFS_REBUILD:-}" != "1" ] \
  && [ -x "${ROOTFS_DIR}/usr/local/bin/k3s" ] \
  && [ -f "${ROOTFS_DIR}/opt/openshell/.initialized" ]; then
  echo "using existing gateway rootfs at ${ROOTFS_DIR}"
  exit 0
fi

exec crates/openshell-vm/scripts/build-rootfs.sh
