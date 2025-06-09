// CLASSIFICATION: COMMUNITY
// Filename: status.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
)

// StatusResponse is returned by the /api/status endpoint.
type StatusResponse struct {
	Uptime  string `json:"uptime"`
	Status  string `json:"status"`
	Role    string `json:"role"`
	Workers int    `json:"workers"`
}

// Status writes static health data as JSON.
func Status(w http.ResponseWriter, r *http.Request) {
	resp := StatusResponse{
		Uptime:  "1h",
		Status:  "ok",
		Role:    "Queen",
		Workers: 3,
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(resp)
}
