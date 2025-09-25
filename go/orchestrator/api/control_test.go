// CLASSIFICATION: COMMUNITY
// Filename: control_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-21
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package api

import (
	"bytes"
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"
)

type noopController struct{}

func (noopController) Execute(_ context.Context, _ ControlRequest) error { return nil }

func TestRoleAuthorizerAllowsConfiguredRoles(t *testing.T) {
	authorizer := NewRoleAuthorizer([]string{"QueenPrimary"})
	req := httptest.NewRequest(http.MethodPost, "/api/control", bytes.NewBufferString(`{"command":"assign-role","role":"QueenPrimary"}`))
	recorder := httptest.NewRecorder()
	Control(noopController{}, authorizer).ServeHTTP(recorder, req)
	if recorder.Code != http.StatusOK {
		t.Fatalf("expected OK, got %d", recorder.Code)
	}
}

func TestRoleAuthorizerRejectsUnauthorizedRole(t *testing.T) {
	authorizer := NewRoleAuthorizer([]string{"QueenPrimary"})
	req := httptest.NewRequest(http.MethodPost, "/api/control", bytes.NewBufferString(`{"command":"assign-role","role":"DroneWorker"}`))
	recorder := httptest.NewRecorder()
	Control(noopController{}, authorizer).ServeHTTP(recorder, req)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d", recorder.Code)
	}
}

func TestControlPassesThroughOtherErrors(t *testing.T) {
	errController := controllerFunc(func(context.Context, ControlRequest) error { return errors.New("boom") })
	req := httptest.NewRequest(http.MethodPost, "/api/control", bytes.NewBufferString(`{"command":"assign-role","role":"QueenPrimary"}`))
	recorder := httptest.NewRecorder()
	Control(errController, nil).ServeHTTP(recorder, req)
	if recorder.Code != http.StatusBadGateway {
		t.Fatalf("expected 502, got %d", recorder.Code)
	}
}

type controllerFunc func(context.Context, ControlRequest) error

func (fn controllerFunc) Execute(ctx context.Context, req ControlRequest) error { return fn(ctx, req) }
