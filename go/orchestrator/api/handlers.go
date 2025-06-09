// CLASSIFICATION: COMMUNITY
// Filename: handlers.go v0.1
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

// Control accepts a JSON command and returns 200 OK.
func Control(w http.ResponseWriter, r *http.Request) {
	var cmd struct {
		Command string `json:"command"`
	}
	if err := json.NewDecoder(r.Body).Decode(&cmd); err != nil {
		http.Error(w, "invalid json", http.StatusBadRequest)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]string{"status": "ack"})
}
