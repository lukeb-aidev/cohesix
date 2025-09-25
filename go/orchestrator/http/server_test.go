// CLASSIFICATION: COMMUNITY
// Filename: server_test.go v0.2
// Author: Lukas Bower
// Date Modified: 2029-02-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http_test

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"

	"cohesix/internal/orchestrator/api"
	orch "cohesix/internal/orchestrator/http"
	"cohesix/internal/orchestrator/rpc"
	"github.com/go-chi/chi/v5"
	"golang.org/x/time/rate"
)

type testGateway struct {
	state       *rpc.ClusterStateResponse
	executeErr  error
	lastRequest api.ControlRequest
	calls       int
}

func newTestGateway() *testGateway {
	return &testGateway{
		state: &rpc.ClusterStateResponse{
			QueenId:        "queen-primary",
			GeneratedAt:    42,
			TimeoutSeconds: 5,
			Workers: []*rpc.WorkerStatus{
				{
					WorkerId:     "worker-a",
					Role:         "DroneWorker",
					Status:       "ready",
					Ip:           "10.0.0.10",
					Trust:        "green",
					BootTs:       1,
					LastSeen:     2,
					Capabilities: []string{"cuda"},
					Gpu: &rpc.GpuTelemetry{
						PerfWatt:     12.5,
						MemTotal:     1024,
						MemFree:      512,
						LastTemp:     50,
						GpuCapacity:  100,
						CurrentLoad:  80,
						LatencyScore: 3,
					},
				},
			},
		},
	}
}

func (g *testGateway) Execute(ctx context.Context, req api.ControlRequest) error {
	g.calls++
	g.lastRequest = req
	return g.executeErr
}

func (g *testGateway) FetchClusterState(ctx context.Context) (*rpc.ClusterStateResponse, error) {
	return g.state, nil
}

func newRouter(t *testing.T, logPath string) (http.Handler, *testGateway) {
	t.Helper()
	cfg := orch.Config{StaticDir: "../../../static", LogFile: logPath, Dev: true}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	return srv.Router(), gateway
}

func TestBootServesRoot(t *testing.T) {
	router, _ := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/")
	if err != nil {
		t.Fatalf("get root: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestStatusEndpoint(t *testing.T) {
	router, gateway := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/api/status")
	if err != nil {
		t.Fatalf("get status: %v", err)
	}
	var m map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&m); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if m["status"] != "ok" || m["role"] == nil || m["uptime"] == nil || m["workers"] == nil {
		t.Fatalf("missing fields: %v", m)
	}
	if got := m["queen_id"]; got != gateway.state.GetQueenId() {
		t.Fatalf("unexpected queen_id: %v", got)
	}
}

func TestControlEndpoint(t *testing.T) {
	router, gateway := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":"assign-role","worker_id":"worker-a","role":"QueenPrimary"}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
	var ack map[string]string
	if err := json.NewDecoder(resp.Body).Decode(&ack); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if ack["status"] != "ack" {
		t.Fatalf("unexpected response: %v", ack)
	}
	if gateway.lastRequest.Command != "assign-role" || gateway.lastRequest.Role != "QueenPrimary" {
		t.Fatalf("unexpected gateway request: %+v", gateway.lastRequest)
	}
}

func TestControlEndpointUpdateTrust(t *testing.T) {
	router, gateway := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":"update-trust","worker_id":"worker-a","trust_level":"amber"}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
	if gateway.lastRequest.Command != "update-trust" || gateway.lastRequest.TrustLevel != "amber" {
		t.Fatalf("unexpected request: %+v", gateway.lastRequest)
	}
}

func TestControlEndpointSchedule(t *testing.T) {
	router, gateway := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":"schedule","agent_id":"cohrun-test","require_gpu":true}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
	if gateway.lastRequest.Command != "schedule" || gateway.lastRequest.AgentID != "cohrun-test" {
		t.Fatalf("unexpected request: %+v", gateway.lastRequest)
	}
	if gateway.lastRequest.RequireGPU == nil || !*gateway.lastRequest.RequireGPU {
		t.Fatalf("expected require_gpu to propagate")
	}
}

func TestControlEndpoint_BadJSON(t *testing.T) {
	router, _ := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	if resp.StatusCode != http.StatusBadRequest {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestStaticFileServed(t *testing.T) {
	router, _ := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/static/index.html")
	if err != nil {
		t.Fatalf("get static: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
	buf := new(bytes.Buffer)
	if _, err := buf.ReadFrom(resp.Body); err != nil {
		t.Fatalf("read body: %v", err)
	}
	if !bytes.Contains(buf.Bytes(), []byte("<!DOCTYPE html>")) {
		t.Fatalf("unexpected body")
	}
}

func TestMetricsEndpoint(t *testing.T) {
	router, _ := newRouter(t, "")
	ts := httptest.NewServer(router)
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/api/metrics")
	if err != nil {
		t.Fatalf("get metrics: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
	var m map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&m); err != nil {
		t.Fatalf("decode: %v", err)
	}
	keys := []string{"requests_total", "start_time_seconds", "active_sessions", "control_limit_per_minute", "control_burst_tokens", "control_tokens_available", "control_allowed_total", "control_denied_total"}
	for _, key := range keys {
		if _, ok := m[key]; !ok {
			t.Fatalf("missing field %s: %v", key, m)
		}
	}
}

func TestMetricsReportRateLimitCounters(t *testing.T) {
	cfg := orch.Config{
		StaticDir:    "../../../static",
		AuthUser:     "user",
		AuthPass:     "pass",
		AllowedRoles: []string{"QueenPrimary"},
		ControlRate:  rate.Every(time.Minute),
		ControlBurst: 1,
	}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	ts := httptest.NewServer(srv.Router())
	defer ts.Close()

	request := func() *http.Response {
		buf := bytes.NewBufferString(`{"command":"assign-role","worker_id":"worker-a","role":"QueenPrimary"}`)
		req, err := http.NewRequest(http.MethodPost, ts.URL+"/api/control", buf)
		if err != nil {
			t.Fatalf("request: %v", err)
		}
		req.SetBasicAuth("user", "pass")
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("post: %v", err)
		}
		return resp
	}

	resp := request()
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("expected 200, got %d", resp.StatusCode)
	}
	resp.Body.Close()

	resp = request()
	if resp.StatusCode != http.StatusTooManyRequests {
		t.Fatalf("expected 429, got %d", resp.StatusCode)
	}
	resp.Body.Close()

	metricsResp, err := http.Get(ts.URL + "/api/metrics")
	if err != nil {
		t.Fatalf("metrics: %v", err)
	}
	defer metricsResp.Body.Close()
	var metrics map[string]any
	if err := json.NewDecoder(metricsResp.Body).Decode(&metrics); err != nil {
		t.Fatalf("decode metrics: %v", err)
	}
	if got := metrics["control_allowed_total"].(float64); got < 1 {
		t.Fatalf("expected allowed total >= 1, got %v", got)
	}
	if got := metrics["control_denied_total"].(float64); got < 1 {
		t.Fatalf("expected denied total >= 1, got %v", got)
	}
}

func TestServerStart(t *testing.T) {
	cfg := orch.Config{Port: 0, StaticDir: "../../../static", Dev: true}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		time.Sleep(100 * time.Millisecond)
		cancel()
	}()
	if err := srv.Start(ctx); err != nil && err != http.ErrServerClosed {
		t.Fatalf("start: %v", err)
	}
}

func TestAccessLogging(t *testing.T) {
	dir := t.TempDir()
	logPath := dir + "/access.log"
	router, _ := newRouter(t, logPath)
	ts := httptest.NewServer(router)
	defer ts.Close()
	if _, err := http.Get(ts.URL + "/api/status"); err != nil {
		t.Fatalf("get status: %v", err)
	}
	data, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatalf("read log: %v", err)
	}
	if !bytes.Contains(data, []byte("/api/status")) {
		t.Fatalf("log missing entry")
	}
}

func TestDevModeDisablesAuth(t *testing.T) {
	cfg := orch.Config{StaticDir: "../../../static", Dev: true, AuthUser: "a", AuthPass: "b"}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	ts := httptest.NewServer(srv.Router())
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":"noop"}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestRecoverMiddleware(t *testing.T) {
	cfg := orch.Config{StaticDir: "../../../static", Dev: true}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	srv.Router().(*chi.Mux).Get("/panic", func(w http.ResponseWriter, r *http.Request) { panic("boom") })
	ts := httptest.NewServer(srv.Router())
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/panic")
	if err != nil {
		t.Fatalf("get: %v", err)
	}
	if resp.StatusCode != http.StatusInternalServerError {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestControlRequiresAuth(t *testing.T) {
	cfg := orch.Config{StaticDir: "../../../static", AuthUser: "queen", AuthPass: "secret"}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	ts := httptest.NewServer(srv.Router())
	defer ts.Close()

	buf := bytes.NewBufferString(`{"command":"assign-role","worker_id":"worker-a","role":"QueenPrimary"}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	if resp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", resp.StatusCode)
	}
}

func TestControlRejectsUnauthorizedRoles(t *testing.T) {
	cfg := orch.Config{StaticDir: "../../../static", AuthUser: "queen", AuthPass: "secret", AllowedRoles: []string{"QueenPrimary"}}
	gateway := newTestGateway()
	cfg.Controller = gateway
	cfg.ClusterClient = gateway
	srv, err := orch.New(cfg)
	if err != nil {
		t.Fatalf("new server: %v", err)
	}
	ts := httptest.NewServer(srv.Router())
	defer ts.Close()

	buf := bytes.NewBufferString(`{"command":"assign-role","worker_id":"worker-a","role":"DroneWorker"}`)
	req, err := http.NewRequest(http.MethodPost, ts.URL+"/api/control", buf)
	if err != nil {
		t.Fatalf("request: %v", err)
	}
	req.SetBasicAuth("queen", "secret")
	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusForbidden {
		t.Fatalf("expected 403, got %d", resp.StatusCode)
	}
	if gateway.calls != 0 {
		t.Fatalf("expected controller not invoked, got %d", gateway.calls)
	}
}
