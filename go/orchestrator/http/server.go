// CLASSIFICATION: COMMUNITY
// Filename: server.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"os"
	"time"

	"cohesix/internal/orchestrator/api"
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
	ctrl   api.Controller
	log    api.Logger
	router *chi.Mux
}

// New returns an initialized server.
func New(cfg Config, ctrl api.Controller, log api.Logger) *Server {
	r := routes(cfg, ctrl, log)
	return &Server{cfg: cfg, ctrl: ctrl, log: log, router: r}
}

// Router returns the underlying router, useful for tests.
func (s *Server) Router() http.Handler {
	return s.router
}

func accessLogger(path string, log api.Logger) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
		if err != nil {
			if log != nil {
				log.Printf("open log: %v", err)
			}
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
	if s.log != nil {
		s.log.Printf("GUI orchestrator listening on %s", s.Addr())
	}
	return srv.ListenAndServe()
}
