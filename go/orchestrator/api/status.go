// CLASSIFICATION: COMMUNITY
// Filename: status.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
)

// Status returns dummy telemetry information as JSON.
func Status(w http.ResponseWriter, r *http.Request) {
	resp := map[string]any{
		"uptime":  "1h",
		"status":  "ok",
		"role":    "Queen",
		"workers": 3,
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}
