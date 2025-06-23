// CLASSIFICATION: COMMUNITY
// Filename: signal_unix.go v0.2
// Author: Lukas Bower
// Date Modified: 2026-07-26
// License: SPDX-License-Identifier: MIT OR Apache-2.0
// Only build on non-Plan 9 systems
//go:build !plan9

package main

import (
	"context"
	"os/signal"
	"syscall"
)

func newSignalContext(ctx context.Context) context.Context {
	c, _ := signal.NotifyContext(ctx, syscall.SIGINT, syscall.SIGTERM)
	return c
}
