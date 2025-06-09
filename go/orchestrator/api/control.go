// CLASSIFICATION: COMMUNITY
// Filename: control.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
)

// ControlRequest is accepted by the /api/control endpoint.
type ControlRequest struct {
	Command string `json:"command"`
}

// AckResponse is returned by successful control calls.
type AckResponse struct {
	Status string `json:"status"`
}

// Control processes a control command and returns an acknowledgement.
func Control(w http.ResponseWriter, r *http.Request) {
	var cmd ControlRequest
	if err := json.NewDecoder(r.Body).Decode(&cmd); err != nil {
		http.Error(w, "invalid json", http.StatusBadRequest)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(AckResponse{Status: "ack"})
}
