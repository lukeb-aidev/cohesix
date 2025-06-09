// CLASSIFICATION: COMMUNITY
// Filename: server.go v0.2
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
	"sync/atomic"
	"time"

	"github.com/fsnotify/fsnotify"

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
	Dev       bool
}

// Server wraps the HTTP server and router.
type Server struct {
	cfg      Config
	router   *chi.Mux
	start    time.Time
	reqCnt   uint64
	sessions int64
}

// New returns an initialized server.
func New(cfg Config) *Server {
	s := &Server{cfg: cfg, router: chi.NewRouter(), start: time.Now()}
	if cfg.LogFile != "" {
		s.router.Use(accessLogger(cfg.LogFile))
	}
	s.router.Use(s.requestCounter)

	s.router.Get("/api/status", api.Status)
	s.router.Post("/api/control", api.Control)
	s.router.Get("/api/metrics", s.metricsHandler)
	s.router.Handle("/static/*", static.FileHandler(cfg.StaticDir))
	s.router.NotFound(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.ServeFile(w, r, cfg.StaticDir+"/index.html")
	}))

	return s
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
	srv.ConnState = func(c net.Conn, st http.ConnState) {
		switch st {
		case http.StateNew:
			atomic.AddInt64(&s.sessions, 1)
		case http.StateClosed, http.StateHijacked:
			atomic.AddInt64(&s.sessions, -1)
		}
	}
	if s.cfg.Dev {
		go watchStatic(s.cfg.StaticDir)
	}
	go func() {
		<-ctx.Done()
		ctxTo, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		srv.Shutdown(ctxTo)
	}()
	log.Printf("GUI orchestrator listening on %s", s.Addr())
	return srv.ListenAndServe()
}

func (s *Server) requestCounter(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddUint64(&s.reqCnt, 1)
		next.ServeHTTP(w, r)
	})
}

func (s *Server) metricsHandler(w http.ResponseWriter, r *http.Request) {
	metrics := fmt.Sprintf(`# HELP requests_total total requests
# TYPE requests_total counter
requests_total %d
# HELP start_time_seconds last restart time
# TYPE start_time_seconds gauge
start_time_seconds %d
# HELP active_sessions current sessions
# TYPE active_sessions gauge
active_sessions %d
`, atomic.LoadUint64(&s.reqCnt), s.start.Unix(), atomic.LoadInt64(&s.sessions))
	w.Header().Set("Content-Type", "text/plain; version=0.0.4")
	w.Write([]byte(metrics))
}

func watchStatic(dir string) {
	w, err := fsnotify.NewWatcher()
	if err != nil {
		log.Printf("watch error: %v", err)
		return
	}
	if err := w.Add(dir); err != nil {
		log.Printf("watch add: %v", err)
		return
	}
	go func() {
		for {
			select {
			case ev, ok := <-w.Events:
				if !ok {
					return
				}
				log.Printf("static changed: %s", ev.Name)
			case err, ok := <-w.Errors:
				if !ok {
					return
				}
				log.Printf("watch error: %v", err)
			}
		}
	}()
}
