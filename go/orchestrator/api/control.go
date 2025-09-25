// CLASSIFICATION: COMMUNITY
// Filename: control.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-21
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

// ControlAuthorizer determines whether a control request is permitted.
type ControlAuthorizer interface {
	Authorize(ControlRequest) error
}

// ErrUnauthorizedRole signals that the requested role is not permitted.
var ErrUnauthorizedRole = errors.New("unauthorized role")

// Control handles POST /api/control requests.
func Control(ctrl Controller, authorizer ControlAuthorizer) http.HandlerFunc {
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
		if authorizer != nil {
			if err := authorizer.Authorize(req); err != nil {
				status := http.StatusBadRequest
				if errors.Is(err, ErrUnauthorizedRole) {
					status = http.StatusForbidden
				}
				http.Error(w, err.Error(), status)
				return
			}
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

// RoleAuthorizer enforces command execution against an allowed role set.
type RoleAuthorizer struct {
	allowed map[string]struct{}
}

// NewRoleAuthorizer constructs a role-based authorizer. An empty role slice
// permits all role transitions.
func NewRoleAuthorizer(roles []string) *RoleAuthorizer {
	if len(roles) == 0 {
		return &RoleAuthorizer{}
	}
	allowed := make(map[string]struct{}, len(roles))
	for _, role := range roles {
		role = strings.TrimSpace(role)
		if role == "" {
			continue
		}
		allowed[role] = struct{}{}
	}
	return &RoleAuthorizer{allowed: allowed}
}

// Authorize checks whether the requested control operation targets an allowed
// role. Only assign-role commands carry a target role and therefore require
// validation.
func (a *RoleAuthorizer) Authorize(req ControlRequest) error {
	if a == nil || len(a.allowed) == 0 {
		return nil
	}
	if strings.TrimSpace(req.Command) != "assign-role" {
		return nil
	}
	role := strings.TrimSpace(req.Role)
	if role == "" {
		return nil
	}
	if _, ok := a.allowed[role]; ok {
		return nil
	}
	return ErrUnauthorizedRole
}
