// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.5
// Author: Lukas Bower
// Date Modified: 2029-02-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
	"context"
	"encoding/json"
	"flag"
	"log"
	"net/http"
	"os"
	"time"

	orchestrator "cohesix/internal/orchestrator/http"
)

type credentials struct {
	User     string   `json:"user"`
	Pass     string   `json:"pass"`
	Roles    []string `json:"roles"`
	TLSCert  string   `json:"tls_cert"`
	TLSKey   string   `json:"tls_key"`
	ClientCA string   `json:"client_ca"`
}

func loadCreds(path string) (credentials, error) {
	f, err := os.Open(path)
	if err != nil {
		return credentials{}, err
	}
	defer f.Close()
	var c credentials
	if err := json.NewDecoder(f).Decode(&c); err != nil {
		return credentials{}, err
	}
	return c, nil
}

func main() {
	bind := flag.String("bind", "127.0.0.1", "bind address")
	port := flag.Int("port", 8888, "listen port")
	staticDir := flag.String("static-dir", "static", "directory for static files")
	logFile := flag.String("log-file", "/srv/trace/gui_access.log", "access log file")
	dev := flag.Bool("dev", false, "enable developer mode")
	flag.Parse()

	cfg := orchestrator.Config{
		Bind:      *bind,
		Port:      *port,
		StaticDir: *staticDir,
		LogFile:   *logFile,
		Dev:       *dev,
	}
	cfg.RPCTimeout = 5 * time.Second

	if !cfg.Dev {
		if creds, err := loadCreds("/srv/orch_user.json"); err == nil {
			cfg.AuthUser = creds.User
			cfg.AuthPass = creds.Pass
			if len(creds.Roles) > 0 {
				cfg.AllowedRoles = append([]string(nil), creds.Roles...)
			}
			if creds.TLSCert != "" {
				cfg.TLSCertFile = creds.TLSCert
			}
			if creds.TLSKey != "" {
				cfg.TLSKeyFile = creds.TLSKey
			}
			if creds.ClientCA != "" {
				cfg.TLSClientCA = creds.ClientCA
			}
		} else {
			log.Printf("warning: could not load creds: %v", err)
		}
	}

	if *dev {
		log.SetFlags(log.LstdFlags | log.Lshortfile)
	}

	srv, err := orchestrator.New(cfg)
	if err != nil {
		log.Fatalf("initialise orchestrator: %v", err)
	}
	ctx, cancel := newSignalContext(context.Background())
	defer cancel()
	if err := srv.Start(ctx); err != nil && err != http.ErrServerClosed {
		log.Fatalf("server error: %v", err)
	}
}
