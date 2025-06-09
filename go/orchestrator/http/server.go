// CLASSIFICATION: COMMUNITY
// Filename: server.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-21
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
	"golang.org/x/time/rate"

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
	cfg            Config
	router         *chi.Mux
	controlLimiter *rate.Limiter
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
	r.Use(recoverMiddleware())

	srv := &Server{
		cfg:            cfg,
		router:         r,
		controlLimiter: rate.NewLimiter(rate.Every(time.Minute/10), 10),
	}

	r.Get("/api/status", api.Status)
	ctrl := http.HandlerFunc(api.Control)
	handler := rateLimitMiddleware(srv.controlLimiter)(ctrl)
	if cfg.AuthUser != "" {
		handler = basicAuthMiddleware(cfg.AuthUser, cfg.AuthPass)(handler)
	}
	r.Post("/api/control", func(w http.ResponseWriter, r *http.Request) {
		handler.ServeHTTP(w, r)
	})
	r.Handle("/static/*", static.FileHandler(cfg.StaticDir))
	r.NotFound(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.ServeFile(w, r, cfg.StaticDir+"/index.html")
	}))

	return srv
}

// Router returns the underlying router, useful for tests.
func (s *Server) Router() http.Handler {
	return s.router
}

func accessLogger(path string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
		if err != nil {
			log.Printf("open log: %v", err)
			return next
		}
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			next.ServeHTTP(w, r)
			rec := r.RemoteAddr + " " + r.Method + " " + r.URL.Path + "\n"
			if _, err := f.Write([]byte(rec)); err != nil {
				log.Printf("access log write: %v", err)
			}
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
func recoverMiddleware() func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			defer func() {
				if err := recover(); err != nil {
					log.Printf("panic: %v", err)
					http.Error(w, "internal server error", http.StatusInternalServerError)
				}
			}()
			next.ServeHTTP(w, r)
		})
	}
}

func basicAuthMiddleware(user, pass string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			u, p, ok := r.BasicAuth()
			if !ok || u != user || p != pass {
				w.Header().Set("WWW-Authenticate", "Basic realm=restricted")
				http.Error(w, "unauthorized", http.StatusUnauthorized)
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}

func rateLimitMiddleware(l *rate.Limiter) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if !l.Allow() {
				http.Error(w, "too many requests", http.StatusTooManyRequests)
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}
