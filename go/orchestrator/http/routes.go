// CLASSIFICATION: COMMUNITY
// Filename: routes.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-20
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package http

import (
	stdhttp "net/http"

	"cohesix/internal/orchestrator/api"
	"cohesix/internal/orchestrator/static"
	"github.com/go-chi/chi/v5"
)

func routes(cfg Config, ctrl api.Controller, log api.Logger) *chi.Mux {
	r := chi.NewRouter()
	if cfg.LogFile != "" {
		r.Use(accessLogger(cfg.LogFile, log))
	}

	r.Get("/api/status", api.Status)
	r.Post("/api/control", api.Control(ctrl, log))
	r.Handle("/static/*", static.FileHandler(cfg.StaticDir))
	r.NotFound(stdhttp.HandlerFunc(func(w stdhttp.ResponseWriter, r *stdhttp.Request) {
		stdhttp.ServeFile(w, r, cfg.StaticDir+"/index.html")
	}))
	return r
}
