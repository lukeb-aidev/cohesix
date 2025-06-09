// CLASSIFICATION: COMMUNITY
// Filename: routes.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"net/http"
	"time"

	"cohesix/internal/orchestrator/api"
	"cohesix/internal/orchestrator/static"
	"github.com/go-chi/chi/v5"
	"golang.org/x/time/rate"
)

// newRouter configures all routes and middleware.
func newRouter(s *Server) *chi.Mux {
	r := chi.NewRouter()
	r.Use(recoverMiddleware())
	if s.cfg.LogFile != "" {
		r.Use(accessLogger(s.cfg.LogFile))
	}
	r.Use(s.requestCounter)

	if !s.cfg.Dev && s.cfg.AuthUser != "" {
		r.Use(basicAuthMiddleware(s.cfg.AuthUser, s.cfg.AuthPass))
	}

	if !s.cfg.Dev {
		s.controlLimiter = rate.NewLimiter(rate.Every(time.Minute/10), 10)
	} else {
		s.controlLimiter = rate.NewLimiter(rate.Inf, 0)
	}

	r.Get("/api/status", api.Status)
	r.Get("/api/metrics", s.metricsHandler)

	ctrl := rateLimitMiddleware(s.controlLimiter)(http.HandlerFunc(api.Control))
	r.Post("/api/control", func(w http.ResponseWriter, r *http.Request) {
		ctrl.ServeHTTP(w, r)
	})

	r.Handle("/static/*", static.FileHandler(s.cfg.StaticDir))
	r.NotFound(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.ServeFile(w, r, s.cfg.StaticDir+"/index.html")
	}))
	return r
}
