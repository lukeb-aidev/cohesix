// CLASSIFICATION: COMMUNITY
// Filename: server.go v0.3
// Author: Lukas Bower
// Date Modified: 2029-02-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"os"
	"strings"
	"sync/atomic"
	"time"

	"github.com/fsnotify/fsnotify"
	"golang.org/x/time/rate"

	"cohesix/internal/orchestrator/api"
	"github.com/go-chi/chi/v5"
)

// Config holds server configuration.
type Config struct {
	Bind          string
	Port          int
	StaticDir     string
	AuthUser      string
	AuthPass      string
	LogFile       string
	Dev           bool
	GRPCEndpoint  string
	RPCTimeout    time.Duration
	Controller    api.Controller
	ClusterClient api.ClusterStateClient
	AllowedRoles  []string
	TLSCertFile   string
	TLSKeyFile    string
	TLSClientCA   string
	ControlRate   rate.Limit
	ControlBurst  int
}

// Server wraps the HTTP server and router.
type Server struct {
	cfg            Config
	router         *chi.Mux
	start          time.Time
	reqCnt         uint64
	sessions       int64
	controlLimiter *rate.Limiter
	logger         Logger
	controller     api.Controller
	clusterClient  api.ClusterStateClient
	closers        []io.Closer
	roleAuthorizer api.ControlAuthorizer
	controlAllowed uint64
	controlDenied  uint64
	tlsConfig      *tls.Config
}

// New returns an initialized server.
func New(cfg Config) (*Server, error) {
	if !cfg.Dev {
		if strings.TrimSpace(cfg.AuthUser) == "" || strings.TrimSpace(cfg.AuthPass) == "" {
			return nil, fmt.Errorf("basic auth credentials are required outside dev mode")
		}
	}

	s := &Server{
		cfg:    cfg,
		router: chi.NewRouter(),
		start:  time.Now(),
		logger: log.Default(),
	}

	limit := cfg.ControlRate
	if limit <= 0 {
		limit = rate.Every(time.Minute / 60)
	}
	burst := cfg.ControlBurst
	if burst <= 0 {
		burst = 60
	}
	s.controlLimiter = rate.NewLimiter(limit, burst)
	if cfg.Controller != nil {
		s.controller = cfg.Controller
	}
	if cfg.ClusterClient != nil {
		s.clusterClient = cfg.ClusterClient
	}

	roles := cfg.AllowedRoles
	if len(roles) == 0 {
		roles = []string{"QueenPrimary", "RegionalQueen", "BareMetalQueen"}
	}
	s.roleAuthorizer = api.NewRoleAuthorizer(roles)

	if s.controller == nil || s.clusterClient == nil {
		var (
			gateway *api.GRPCGateway
			err     error
		)
		ctx := context.Background()
		timeout := cfg.RPCTimeout
		if cfg.GRPCEndpoint != "" {
			gateway, err = api.NewGRPCGateway(ctx, cfg.GRPCEndpoint, timeout)
		} else {
			gateway, err = api.NewGRPCGatewayFromEnv(ctx, timeout)
		}
		if err != nil {
			return nil, err
		}
		s.controller = gateway
		s.clusterClient = gateway
		s.closers = append(s.closers, gateway)
	}
	if cfg.LogFile != "" {
		s.router.Use(accessLogger(cfg.LogFile))
	}
	if cfg.TLSCertFile != "" || cfg.TLSKeyFile != "" || cfg.TLSClientCA != "" {
		if cfg.TLSCertFile == "" || cfg.TLSKeyFile == "" {
			return nil, fmt.Errorf("tls_cert_file and tls_key_file must both be provided for TLS")
		}
		tlsCfg, err := buildTLSConfig(cfg.TLSCertFile, cfg.TLSKeyFile, cfg.TLSClientCA)
		if err != nil {
			return nil, err
		}
		s.tlsConfig = tlsCfg
	}
	s.initRoutes()
	return s, nil
}

// Router returns the underlying router, useful for tests.
func (s *Server) Router() http.Handler { return s.router }

func accessLogger(path string) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
		if err != nil {
			log.Printf("open log: %v", err)
			return next
		}
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			next.ServeHTTP(w, r)
			rec := fmt.Sprintf("%s %s %s\n", r.RemoteAddr, r.Method, r.URL.Path)
			if _, err := f.Write([]byte(rec)); err != nil {
				log.Printf("access log write: %v", err)
			}
		})
	}
}

// Addr returns the listening address.
func (s *Server) Addr() string { return net.JoinHostPort(s.cfg.Bind, fmt.Sprint(s.cfg.Port)) }

// Start begins serving until ctx is done.
func (s *Server) Start(ctx context.Context) error {
	srv := &http.Server{Addr: s.Addr(), Handler: s.router, TLSConfig: s.tlsConfig}
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
		for _, closer := range s.closers {
			if err := closer.Close(); err != nil {
				s.logger.Printf("close error: %v", err)
			}
		}
		ctxTo, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		srv.Shutdown(ctxTo)
	}()
	s.logger.Printf("GUI orchestrator listening on %s", s.Addr())
	var err error
	if s.tlsConfig != nil {
		err = srv.ListenAndServeTLS("", "")
	} else {
		err = srv.ListenAndServe()
	}
	if err == http.ErrServerClosed {
		return nil
	}
	return err
}

func (s *Server) requestCounter(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		atomic.AddUint64(&s.reqCnt, 1)
		next.ServeHTTP(w, r)
	})
}

type metricsResponse struct {
	RequestsTotal          uint64  `json:"requests_total"`
	StartTime              int64   `json:"start_time_seconds"`
	ActiveSessions         int64   `json:"active_sessions"`
	ControlLimitPerMinute  float64 `json:"control_limit_per_minute"`
	ControlBurstTokens     int     `json:"control_burst_tokens"`
	ControlTokensAvailable float64 `json:"control_tokens_available"`
	ControlAllowedTotal    uint64  `json:"control_allowed_total"`
	ControlDeniedTotal     uint64  `json:"control_denied_total"`
}

func (s *Server) metricsHandler(w http.ResponseWriter, r *http.Request) {
	limit := s.controlLimiter.Limit()
	resp := metricsResponse{
		RequestsTotal:          atomic.LoadUint64(&s.reqCnt),
		StartTime:              s.start.Unix(),
		ActiveSessions:         atomic.LoadInt64(&s.sessions),
		ControlLimitPerMinute:  float64(limit) * 60,
		ControlBurstTokens:     s.controlLimiter.Burst(),
		ControlTokensAvailable: s.controlLimiter.Tokens(),
		ControlAllowedTotal:    atomic.LoadUint64(&s.controlAllowed),
		ControlDeniedTotal:     atomic.LoadUint64(&s.controlDenied),
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(resp)
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

func (s *Server) rateLimitMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if !s.controlLimiter.Allow() {
			atomic.AddUint64(&s.controlDenied, 1)
			http.Error(w, "too many requests", http.StatusTooManyRequests)
			return
		}
		atomic.AddUint64(&s.controlAllowed, 1)
		next.ServeHTTP(w, r)
	})
}

func buildTLSConfig(certFile, keyFile, clientCA string) (*tls.Config, error) {
	certificate, err := tls.LoadX509KeyPair(certFile, keyFile)
	if err != nil {
		return nil, fmt.Errorf("load server certificate: %w", err)
	}
	cfg := &tls.Config{
		MinVersion:   tls.VersionTLS13,
		Certificates: []tls.Certificate{certificate},
	}
	if strings.TrimSpace(clientCA) != "" {
		data, err := os.ReadFile(clientCA)
		if err != nil {
			return nil, fmt.Errorf("read client ca: %w", err)
		}
		pool := x509.NewCertPool()
		if !pool.AppendCertsFromPEM(data) {
			return nil, fmt.Errorf("parse client ca %q", clientCA)
		}
		cfg.ClientAuth = tls.RequireAndVerifyClientCert
		cfg.ClientCAs = pool
	}
	return cfg, nil
}
