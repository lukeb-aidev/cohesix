// CLASSIFICATION: COMMUNITY
// Filename: status.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
	"time"
)

// StatusResponse describes orchestrator state.
type StatusResponse struct {
	Uptime  string `json:"uptime"`
	Status  string `json:"status"`
	Role    string `json:"role"`
	Workers int    `json:"workers"`
}

// Status writes a static status response.
func Status(start time.Time) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		up := time.Since(start).Round(time.Second).String()
		resp := StatusResponse{
			Uptime:  up,
			Status:  "ok",
			Role:    "Queen",
			Workers: 3,
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(resp)
	}
}
