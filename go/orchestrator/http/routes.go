// CLASSIFICATION: COMMUNITY
// Filename: routes.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	"net/http"
	"path"

	"cohesix/internal/orchestrator/api"
	"cohesix/internal/orchestrator/static"
)

func (s *Server) initRoutes() {
	r := s.router
	r.Use(recoverMiddleware())
	r.Use(s.requestCounter)

	r.Get("/api/status", api.Status(s.start))

	handler := http.Handler(api.Control(s.controller))
	if !s.cfg.Dev {
		handler = rateLimitMiddleware(s.controlLimiter)(handler)
		if s.cfg.AuthUser != "" {
			handler = basicAuthMiddleware(s.cfg.AuthUser, s.cfg.AuthPass)(handler)
		}
	}
	r.Post("/api/control", func(w http.ResponseWriter, r *http.Request) { handler.ServeHTTP(w, r) })

	r.Get("/api/metrics", s.metricsHandler)
	r.Handle("/static/*", static.FileHandler(s.cfg.StaticDir))
	r.NotFound(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.ServeFile(w, r, path.Join(s.cfg.StaticDir, "index.html"))
	}))
}
