// CLASSIFICATION: COMMUNITY
// Filename: server.go v0.3
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"net"
	"net/http"
	"os"
	"sync/atomic"
	"time"

	"github.com/fsnotify/fsnotify"
	"golang.org/x/time/rate"
)

// Logger is used for logging within the server.
type Logger interface {
	Printf(string, ...any)
}

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
	cfg            Config
	log            Logger
	router         http.Handler
	start          time.Time
	reqCnt         uint64
	sessions       int64
	controlLimiter *rate.Limiter
}

// New constructs a Server using cfg and logger.
func New(cfg Config, lg Logger) *Server {
	s := &Server{cfg: cfg, log: lg, start: time.Now()}
	s.router = newRouter(s)
	return s
}

// Router returns the underlying router.
func (s *Server) Router() http.Handler { return s.router }

// Addr returns the listening address.
func (s *Server) Addr() string { return net.JoinHostPort(s.cfg.Bind, fmt.Sprint(s.cfg.Port)) }

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
	if s.log != nil {
		s.log.Printf("GUI orchestrator listening on %s", s.Addr())
	} else {
		log.Printf("GUI orchestrator listening on %s", s.Addr())
	}
	return srv.ListenAndServe()
}

func (s *Server) requestCounter(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddUint64(&s.reqCnt, 1)
		next.ServeHTTP(w, r)
	})
}

func (s *Server) metricsHandler(w http.ResponseWriter, r *http.Request) {
	resp := struct {
		Requests uint64 `json:"requests_total"`
		Start    int64  `json:"start_time_seconds"`
		Sessions int64  `json:"active_sessions"`
	}{
		Requests: atomic.LoadUint64(&s.reqCnt),
		Start:    s.start.Unix(),
		Sessions: atomic.LoadInt64(&s.sessions),
	}
	w.Header().Set("Content-Type", "application/json")
	_ = json.NewEncoder(w).Encode(resp)
}

func watchStatic(dir string) {
	w, err := fsnotify.NewWatcher()
	if err != nil {
		log.Printf("watch error: %v", err)
		return
	}
	defer w.Close()
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
