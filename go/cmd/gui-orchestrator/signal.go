// CLASSIFICATION: COMMUNITY
// Filename: signal.go v0.4
// Author: Lukas Bower
// Date Modified: 2026-07-30
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
	"context"
	"os/signal"
	"syscall"
)

func newSignalContext(ctx context.Context) (context.Context, context.CancelFunc) {
	return signal.NotifyContext(ctx, syscall.SIGINT, syscall.SIGTERM)
}
