// CLASSIFICATION: COMMUNITY
// Filename: signal_plan9.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

//go:build plan9

package main

import "context"

func newSignalContext() (context.Context, context.CancelFunc) {
	return context.WithCancel(context.Background())
}
