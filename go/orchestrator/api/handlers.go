// CLASSIFICATION: COMMUNITY
// Filename: handlers.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
)

// Status returns dummy telemetry information as JSON.
type StatusResponse struct {
	Uptime  string `json:"uptime"`
	Status  string `json:"status"`
	Role    string `json:"role"`
	Workers int    `json:"workers"`
}

func Status(w http.ResponseWriter, r *http.Request) {
	resp := StatusResponse{
		Uptime:  "1h",
		Status:  "ok",
		Role:    "Queen",
		Workers: 3,
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
}

// Control accepts a JSON command and returns 200 OK.
type ControlRequest struct {
	Command string `json:"command"`
}

type AckResponse struct {
	Status string `json:"status"`
}

func Control(w http.ResponseWriter, r *http.Request) {
	var cmd ControlRequest
	if err := json.NewDecoder(r.Body).Decode(&cmd); err != nil {
		http.Error(w, "invalid json", http.StatusBadRequest)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(AckResponse{Status: "ack"})
}
