// CLASSIFICATION: COMMUNITY
// Filename: signal_plan9.go v0.3
// Author: Lukas Bower
// Date Modified: 2026-07-27
// License: SPDX-License-Identifier: MIT OR Apache-2.0
// Only build on Plan 9 systems
//go:build plan9

package main

import "context"

func newSignalContext(ctx context.Context) (context.Context, context.CancelFunc) {
	return ctx, func() {}
}
