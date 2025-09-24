// CLASSIFICATION: COMMUNITY
// Filename: control.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-15
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"strings"
)

// ControlRequest is a command sent to the orchestrator.
type ControlRequest struct {
	Command    string `json:"command"`
	WorkerID   string `json:"worker_id,omitempty"`
	Role       string `json:"role,omitempty"`
	TrustLevel string `json:"trust_level,omitempty"`
	AgentID    string `json:"agent_id,omitempty"`
	RequireGPU *bool  `json:"require_gpu,omitempty"`
}

// AckResponse is returned on successful control execution.
type AckResponse struct {
	Status string `json:"status"`
}

// Controller executes orchestration commands.
type Controller interface {
	Execute(ctx context.Context, cmd ControlRequest) error
}

// Control handles POST /api/control requests.
func Control(ctrl Controller) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var req ControlRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "invalid json", http.StatusBadRequest)
			return
		}
		if ctrl == nil {
			http.Error(w, "control unavailable", http.StatusServiceUnavailable)
			return
		}
		req.Command = strings.TrimSpace(req.Command)
		if req.Command == "" {
			http.Error(w, "command required", http.StatusBadRequest)
			return
		}
		if err := ctrl.Execute(r.Context(), req); err != nil {
			var status int
			switch {
			case errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded):
				status = http.StatusGatewayTimeout
			default:
				status = http.StatusBadGateway
			}
			http.Error(w, err.Error(), status)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(AckResponse{Status: "ack"})
	}
}
