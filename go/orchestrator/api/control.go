// CLASSIFICATION: COMMUNITY
// Filename: control.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"context"
	"encoding/json"
	"net/http"
)

// ControlRequest is a command sent to the orchestrator.
type ControlRequest struct {
	Command string `json:"command"`
}

// AckResponse is returned on successful control execution.
type AckResponse struct {
	Status string `json:"status"`
}

// Controller executes orchestration commands.
type Controller interface {
	Execute(ctx context.Context, cmd ControlRequest) error
}

// defaultController is a no-op implementation.
type defaultController struct{}

func (defaultController) Execute(ctx context.Context, cmd ControlRequest) error { return nil }

// DefaultController returns a controller that does nothing.
func DefaultController() Controller { return defaultController{} }

// Control handles POST /api/control requests.
func Control(ctrl Controller) http.HandlerFunc {
	if ctrl == nil {
		ctrl = DefaultController()
	}
	return func(w http.ResponseWriter, r *http.Request) {
		var req ControlRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "invalid json", http.StatusBadRequest)
			return
		}
		if err := ctrl.Execute(r.Context(), req); err != nil {
			http.Error(w, "control error", http.StatusInternalServerError)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(AckResponse{Status: "ack"})
	}
}
