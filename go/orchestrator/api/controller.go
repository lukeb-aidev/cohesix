// CLASSIFICATION: COMMUNITY
// Filename: controller.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

// Controller defines control operations for the orchestrator.
type Controller interface {
	Restart() error
	Shutdown() error
}

// Logger abstracts logging for the orchestrator.
type Logger interface {
	Printf(string, ...any)
}
