// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.5
// Author: Lukas Bower
// Date Modified: 2027-08-04
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
	"context"
	"encoding/json"
	"flag"
	"log"
	"net/http"
	"os"

	orchestrator "cohesix/internal/orchestrator/http"
)

type credentials struct {
	User string `json:"user"`
	Pass string `json:"pass"`
}

func loadCreds(path string) (string, string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", "", err
	}
	defer f.Close()
	var c credentials
	if err := json.NewDecoder(f).Decode(&c); err != nil {
		return "", "", err
	}
	return c.User, c.Pass, nil
}

func main() {
	bind := flag.String("bind", "127.0.0.1", "bind address")
	port := flag.Int("port", 8888, "listen port")
	staticDir := flag.String("static-dir", "static", "directory for static files")
	logFile := flag.String("log-file", "/log/gui_access.log", "access log file")
	dev := flag.Bool("dev", false, "enable developer mode")
	flag.Parse()

	cfg := orchestrator.Config{
		Bind:      *bind,
		Port:      *port,
		StaticDir: *staticDir,
		LogFile:   *logFile,
		Dev:       *dev,
	}

	if !cfg.Dev {
		if u, p, err := loadCreds("/srv/orch_user.json"); err == nil {
			cfg.AuthUser = u
			cfg.AuthPass = p
		} else {
			log.Printf("warning: could not load creds: %v", err)
		}
	}

	if *dev {
		log.SetFlags(log.LstdFlags | log.Lshortfile)
	}

	srv := orchestrator.New(cfg)
	ctx, cancel := newSignalContext(context.Background())
	defer cancel()
	if err := srv.Start(ctx); err != nil && err != http.ErrServerClosed {
		log.Fatalf("server error: %v", err)
	}
}
