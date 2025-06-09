// CLASSIFICATION: COMMUNITY
// Filename: control.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
)

// controlRequest represents a control command.
type controlRequest struct {
	Command string `json:"command"`
}

// Control handles control commands like restart or shutdown.
func Control(ctrl Controller, log Logger) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		var req controlRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "invalid json", http.StatusBadRequest)
			return
		}
		switch req.Command {
		case "restart":
			if err := ctrl.Restart(); err != nil {
				http.Error(w, err.Error(), http.StatusInternalServerError)
				return
			}
			if log != nil {
				log.Printf("restart triggered")
			}
		case "shutdown":
			if err := ctrl.Shutdown(); err != nil {
				http.Error(w, err.Error(), http.StatusInternalServerError)
				return
			}
			if log != nil {
				log.Printf("shutdown triggered")
			}
		default:
			http.Error(w, "unknown command", http.StatusBadRequest)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]string{"status": "ack"})
	}
}
