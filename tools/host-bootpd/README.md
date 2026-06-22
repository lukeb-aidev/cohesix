<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the managed Mac bootpd helper files for Pi 4 direct-link DHCP. -->
<!-- Author: Lukas Bower -->
# Host bootpd Helper

This directory owns the repo-managed Mac `bootpd` helper config for Pi 4 direct-link DHCP on `en8`.

- Managed config and scripts live in `tools/host-bootpd/`.
- Runtime logs and PID files are disposable and live under `out/host-bootpd/`.
- Install or refresh the root LaunchDaemon with:

```sh
sudo zsh tools/host-bootpd/install-root-bootpd.zsh
```

