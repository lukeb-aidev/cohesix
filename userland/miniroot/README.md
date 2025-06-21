// CLASSIFICATION: COMMUNITY
// Filename: README.md v0.2
// Author: Lukas Bower
// Date Modified: 2026-01-20

# Cohesix Miniroot

This directory contains a minimal set of utilities used during early boot and
interactive testing. New commands may be added under `bin/` and additional
configuration can be placed in `etc/`.

The default toolset now includes a simple `init` launcher and a placeholder
`rc` script used by the boot sequence. Temporary runtime files should use the
`tmp/` directory.
