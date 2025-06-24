// CLASSIFICATION: COMMUNITY
// Filename: signal_unix.go v0.3
// Author: Lukas Bower
// Date Modified: 2026-07-27
// License: SPDX-License-Identifier: MIT OR Apache-2.0
// Only build on non-Plan 9 systems
//go:build !plan9

package main

import (
	"context"
	"os/signal"
	"syscall"
)

func newSignalContext(ctx context.Context) (context.Context, context.CancelFunc) {
	return signal.NotifyContext(ctx, syscall.SIGINT, syscall.SIGTERM)
}
