// CLASSIFICATION: COMMUNITY
// Filename: types.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

// Logger abstracts logging for the server.
type Logger interface {
	Printf(format string, v ...interface{})
}
