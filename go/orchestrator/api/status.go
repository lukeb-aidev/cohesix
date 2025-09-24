// CLASSIFICATION: COMMUNITY
// Filename: status.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-15
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"encoding/json"
	"net/http"
	"os"
	"time"

	"cohesix/internal/orchestrator/rpc"
)

// StatusResponse describes orchestrator state.
type StatusResponse struct {
	Uptime         string          `json:"uptime"`
	Status         string          `json:"status"`
	Role           string          `json:"role"`
	QueenID        string          `json:"queen_id"`
	Workers        int             `json:"workers"`
	GeneratedAt    uint64          `json:"generated_at"`
	TimeoutSeconds uint32          `json:"timeout_seconds"`
	WorkerStatuses []WorkerSummary `json:"worker_statuses"`
}

// WorkerSummary provides a JSON-friendly worker snapshot.
type WorkerSummary struct {
	WorkerID     string      `json:"worker_id"`
	Role         string      `json:"role"`
	Status       string      `json:"status"`
	IP           string      `json:"ip"`
	Trust        string      `json:"trust"`
	BootTS       uint64      `json:"boot_ts"`
	LastSeen     uint64      `json:"last_seen"`
	Capabilities []string    `json:"capabilities"`
	GPU          *GPUSummary `json:"gpu,omitempty"`
}

// GPUSummary mirrors RPC GPU telemetry.
type GPUSummary struct {
	PerfWatt     float32 `json:"perf_watt"`
	MemTotal     uint64  `json:"mem_total"`
	MemFree      uint64  `json:"mem_free"`
	LastTemp     uint32  `json:"last_temp"`
	GPUCapacity  uint32  `json:"gpu_capacity"`
	CurrentLoad  uint32  `json:"current_load"`
	LatencyScore uint32  `json:"latency_score"`
}

// Status writes a live status response pulled from the gRPC orchestrator.
func Status(start time.Time, client ClusterStateClient) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if client == nil {
			http.Error(w, "cluster state unavailable", http.StatusServiceUnavailable)
			return
		}
		state, err := client.FetchClusterState(r.Context())
		if err != nil {
			http.Error(w, err.Error(), http.StatusBadGateway)
			return
		}

		var workers []WorkerSummary
		for _, wkr := range state.GetWorkers() {
			workers = append(workers, WorkerSummary{
				WorkerID:     wkr.GetWorkerId(),
				Role:         wkr.GetRole(),
				Status:       wkr.GetStatus(),
				IP:           wkr.GetIp(),
				Trust:        wkr.GetTrust(),
				BootTS:       wkr.GetBootTs(),
				LastSeen:     wkr.GetLastSeen(),
				Capabilities: append([]string(nil), wkr.GetCapabilities()...),
				GPU:          convertGPU(wkr.GetGpu()),
			})
		}

		role := os.Getenv("COHESIX_ROLE")
		if role == "" {
			role = "Queen"
		}
		resp := StatusResponse{
			Uptime:         time.Since(start).Round(time.Second).String(),
			Status:         "ok",
			Role:           role,
			QueenID:        state.GetQueenId(),
			Workers:        len(workers),
			GeneratedAt:    state.GetGeneratedAt(),
			TimeoutSeconds: state.GetTimeoutSeconds(),
			WorkerStatuses: workers,
		}

		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(resp)
	}
}

func convertGPU(src *rpc.GpuTelemetry) *GPUSummary {
	if src == nil {
		return nil
	}
	return &GPUSummary{
		PerfWatt:     src.GetPerfWatt(),
		MemTotal:     src.GetMemTotal(),
		MemFree:      src.GetMemFree(),
		LastTemp:     src.GetLastTemp(),
		GPUCapacity:  src.GetGpuCapacity(),
		CurrentLoad:  src.GetCurrentLoad(),
		LatencyScore: src.GetLatencyScore(),
	}
}
