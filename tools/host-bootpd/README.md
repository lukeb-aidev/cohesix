<!-- Copyright 2026 Lukas Bower -->
<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Purpose: Document the managed Mac bootpd helper files for Pi 4 direct-link DHCP. -->
<!-- Author: Lukas Bower -->
# Host bootpd Helper

This directory owns the repo-managed Mac `bootpd` helper config for Pi 4 direct-link DHCP on `en8`.

- Managed config and scripts remain in `tools/host-bootpd/`.
- Runtime logs and PID files are disposable and live outside the repository at
  `/Users/lukasbower/cohesix/host-bootpd/`.
- The root LaunchDaemon binds that exact runtime path through
  `COHESIX_BOOTPD_RUNTIME_DIR`. The supervisor refuses an unset or different
  binding and refuses to start if both the external directory and legacy
  `out/host-bootpd/` directory exist.
- Install or refresh the root LaunchDaemon with:

```sh
sudo zsh tools/host-bootpd/install-root-bootpd.zsh
```

The installer moves a sole legacy `out/host-bootpd/` directory to the external
location before launching the daemon. If both locations already exist, it
stops without choosing or merging evidence; reconcile them explicitly and run
the installer again.
