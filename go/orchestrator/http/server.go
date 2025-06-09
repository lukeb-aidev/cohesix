// CLASSIFICATION: COMMUNITY
// Filename: server.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"context"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"time"

	"cohesix/internal/orchestrator/api"
	"cohesix/internal/orchestrator/static"
	"github.com/go-chi/chi/v5"
)

// Config holds server configuration.
type Config struct {
	Bind      string
	Port      int
	StaticDir string
	AuthUser  string
	AuthPass  string
	LogFile   string
}

// Server wraps the HTTP server and router.
type Server struct {
	cfg    Config
	router *chi.Mux
}

// New returns an initialized server.
func New(cfg Config) *Server {
	r := chi.NewRouter()
	if cfg.LogFile != "" {
		r.Use(accessLogger(cfg.LogFile))
	}

	r.Get("/api/status", api.Status)
	r.Post("/api/control", api.Control)
	r.Handle("/static/*", static.FileHandler(cfg.StaticDir))
	r.NotFound(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.ServeFile(w, r, cfg.StaticDir+"/index.html")
	}))

	return &Server{cfg: cfg, router: r}
}

// Router returns the underlying router, useful for tests.
func (s *Server) Router() http.Handler {
	return s.router
}

func accessLogger(path string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
		if err != nil {
			log.Printf("open log: %v", err)
			return next
		}
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			next.ServeHTTP(w, r)
			rec := r.RemoteAddr + " " + r.Method + " " + r.URL.Path + "\n"
			f.Write([]byte(rec))
		})
	}
}

// Addr returns the listening address.
func (s *Server) Addr() string {
	return net.JoinHostPort(s.cfg.Bind, fmt.Sprint(s.cfg.Port))
}

// Start begins serving until ctx is done.
func (s *Server) Start(ctx context.Context) error {
	srv := &http.Server{Addr: s.Addr(), Handler: s.router}
	go func() {
		<-ctx.Done()
		ctxTo, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		srv.Shutdown(ctxTo)
	}()
	log.Printf("GUI orchestrator listening on %s", s.Addr())
	return srv.ListenAndServe()
}
