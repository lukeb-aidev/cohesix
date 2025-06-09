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
	"os"
	"testing"
	"time"

	orch "cohesix/internal/orchestrator/http"
)

func newRouter(logPath string) http.Handler {
	cfg := orch.Config{StaticDir: "../../../static", LogFile: logPath}
	srv := orch.New(cfg)
	return srv.Router()
}

func TestBootServesRoot(t *testing.T) {
	ts := httptest.NewServer(newRouter(""))
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
	ts := httptest.NewServer(newRouter(""))
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
}

func TestControlEndpoint(t *testing.T) {
	ts := httptest.NewServer(newRouter(""))
	defer ts.Close()
	buf := bytes.NewBufferString(`{"command":"restart"}`)
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
}

func TestControlEndpoint_BadJSON(t *testing.T) {
	ts := httptest.NewServer(newRouter(""))
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
	ts := httptest.NewServer(newRouter(""))
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

func TestAccessLogging(t *testing.T) {
	dir := t.TempDir()
	logPath := dir + "/access.log"
	ts := httptest.NewServer(newRouter(logPath))
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
