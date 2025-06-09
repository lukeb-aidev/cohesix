// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
	"context"
	"flag"
	"log"
	"net/http"
	"os/signal"
	"syscall"

	orchestrator "cohesix/internal/orchestrator/http"
)

func main() {
	bind := flag.String("bind", "127.0.0.1", "bind address")
	port := flag.Int("port", 8888, "listen port")
	staticDir := flag.String("static-dir", "static", "directory for static files")
	logFile := flag.String("log-file", "/log/gui_access.log", "access log file")
	flag.Parse()

	cfg := orchestrator.Config{
		Bind:      *bind,
		Port:      *port,
		StaticDir: *staticDir,
		LogFile:   *logFile,
	}

	srv := orchestrator.New(cfg)
	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	if err := srv.Start(ctx); err != nil && err != http.ErrServerClosed {
		log.Fatalf("server error: %v", err)
	}
}
