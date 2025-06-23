// CLASSIFICATION: COMMUNITY
// Filename: signal_plan9.go v0.2
// Author: Lukas Bower
// Date Modified: 2026-07-26
// License: SPDX-License-Identifier: MIT OR Apache-2.0
// Only build on Plan 9 systems
//go:build plan9

package main

import "context"

func newSignalContext(ctx context.Context) context.Context {
	return ctx
}
