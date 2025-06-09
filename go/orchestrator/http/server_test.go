// CLASSIFICATION: COMMUNITY
// Filename: server_test.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"
)

func newTestRouter(logPath string) http.Handler {
	cfg := Config{StaticDir: "../../../static", LogFile: logPath}
	srv := New(cfg, nil)
	return srv.Router()
}

func TestBootServesRoot(t *testing.T) {
	ts := httptest.NewServer(newTestRouter(""))
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
	ts := httptest.NewServer(newTestRouter(""))
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
	ts := httptest.NewServer(newTestRouter(""))
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
	ts := httptest.NewServer(newTestRouter(""))
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
	ts := httptest.NewServer(newTestRouter(""))
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
	ts := httptest.NewServer(newTestRouter(""))
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
	cfg := Config{Port: 0, StaticDir: "../../../static"}
	srv := New(cfg, nil)
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
	ts := httptest.NewServer(newTestRouter(logPath))
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

func TestAuthDisabledInDev(t *testing.T) {
	cfg := Config{StaticDir: "../../../static", AuthUser: "u", AuthPass: "p", Dev: true}
	srv := New(cfg, nil)
	ts := httptest.NewServer(srv.Router())
	defer ts.Close()
	for i := 0; i < 20; i++ {
		resp, err := http.Post(ts.URL+"/api/control", "application/json", bytes.NewBufferString(`{"command":"x"}`))
		if err != nil {
			t.Fatalf("post: %v", err)
		}
		if resp.StatusCode != http.StatusOK {
			t.Fatalf("status %d", resp.StatusCode)
		}
	}
}

func TestMetricsJSON(t *testing.T) {
	ts := httptest.NewServer(newTestRouter(""))
	defer ts.Close()
	resp, err := http.Get(ts.URL + "/api/metrics")
	if err != nil {
		t.Fatalf("get metrics: %v", err)
	}
	var m map[string]any
	if err := json.NewDecoder(resp.Body).Decode(&m); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if m["requests_total"] == nil || m["start_time_seconds"] == nil || m["active_sessions"] == nil {
		t.Fatalf("missing fields: %v", m)
	}
}

func TestRecoverMiddleware(t *testing.T) {
	h := recoverMiddleware()(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		panic("boom")
	}))
	rr := httptest.NewRecorder()
	h.ServeHTTP(rr, httptest.NewRequest("GET", "/", nil))
	if rr.Code != http.StatusInternalServerError {
		t.Fatalf("status %d", rr.Code)
	}
}
