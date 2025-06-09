// CLASSIFICATION: COMMUNITY
// Filename: server_test.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http_test

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	orch "cohesix/internal/orchestrator/http"
)

func newRouter() http.Handler {
	cfg := orch.Config{StaticDir: "../../../static"}
	srv := orch.New(cfg)
	return srv.Router()
}

func TestStatusEndpoint(t *testing.T) {
	ts := httptest.NewServer(newRouter())
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/api/status")
	if err != nil {
		t.Fatalf("get status: %v", err)
	}
	var m map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&m); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if m["status"] != "ok" {
		t.Fatalf("unexpected status")
	}
}

func TestControlEndpoint(t *testing.T) {
	ts := httptest.NewServer(newRouter())
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":"restart"}`)
	resp, err := http.Post(ts.URL+"/api/control", "application/json", buf)
	if err != nil {
		t.Fatalf("post control: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestStaticFileServed(t *testing.T) {
	ts := httptest.NewServer(newRouter())
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/static/index.html")
	if err != nil {
		t.Fatalf("get static: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestMetricsEndpoint(t *testing.T) {
	ts := httptest.NewServer(newRouter())
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/api/metrics")
	if err != nil {
		t.Fatalf("get metrics: %v", err)
	}
	if resp.StatusCode != http.StatusOK {
		t.Fatalf("status code: %d", resp.StatusCode)
	}
}

func TestServerStart(t *testing.T) {
	cfg := orch.Config{Port: 0, StaticDir: "../../../static"}
	srv := orch.New(cfg)
	ctx, cancel := context.WithCancel(context.Background())
	go func() {
		time.Sleep(100 * time.Millisecond)
		cancel()
	}()
	if err := srv.Start(ctx); err != nil && err != http.ErrServerClosed {
		t.Fatalf("start: %v", err)
	}
}
