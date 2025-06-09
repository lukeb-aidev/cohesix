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
	"time"

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
}

// Server wraps the HTTP server and router.
type Server struct {
	cfg            Config
	router         *chi.Mux
	controlLimiter *rate.Limiter
}

// New returns an initialized server.
func New(cfg Config) *Server {
	r := chi.NewRouter()
	if cfg.LogFile != "" {
		r.Use(accessLogger(cfg.LogFile))
	}
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
	go func() {
		<-ctx.Done()
		ctxTo, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		srv.Shutdown(ctxTo)
	}()
	log.Printf("GUI orchestrator listening on %s", s.Addr())
	return srv.ListenAndServe()
}

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
