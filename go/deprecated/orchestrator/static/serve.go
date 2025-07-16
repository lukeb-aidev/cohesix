// CLASSIFICATION: COMMUNITY
// Filename: serve.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package static

import (
	"net/http"
)

// FileHandler returns an HTTP handler that serves files from dir.
func FileHandler(dir string) http.Handler {
	fs := http.FileServer(http.Dir(dir))
	return http.StripPrefix("/static/", fs)
}
