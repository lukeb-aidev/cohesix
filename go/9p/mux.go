// CLASSIFICATION: COMMUNITY
// Filename: mux.go v0.2
// Date Modified: 2025-06-25
// Author: Lukas Bower
//
// Concurrent 9P multiplexer used by integration tests. Each service
// registers a name and implements the Handler interface.
package p9

import (
    "context"
    "sync"
)

// Request sent to a handler.
type Request struct {
    Path string
    Data []byte
}

// Handler represents a 9P service.
type Handler interface {
    Handle(path string, data []byte) ([]byte, error)
}

// Mux routes requests to registered handlers.
type Mux struct {
    mu       sync.RWMutex
    services map[string]Handler
}

// NewMux returns an initialised multiplexer.
func NewMux() *Mux {
    return &Mux{services: make(map[string]Handler)}
}

// Register adds a named handler.
func (m *Mux) Register(name string, h Handler) {
    m.mu.Lock()
    defer m.mu.Unlock()
    m.services[name] = h
}

// Serve processes requests from the provided channel until ctx is done.
func (m *Mux) Serve(ctx context.Context, reqCh <-chan Request) {
    for {
        select {
        case <-ctx.Done():
            return
        case r := <-reqCh:
            go func(r Request) {
                resp := m.Handle(r.Path, r.Data)
                _ = resp // placeholder for integration
            }(r)
        }
    }
}

// Handle routes a request synchronously and returns the result.
func (m *Mux) Handle(path string, data []byte) []byte {
    m.mu.RLock()
    defer m.mu.RUnlock()
    for prefix, h := range m.services {
        if len(path) >= len(prefix) && path[:len(prefix)] == prefix {
            out, _ := h.Handle(path[len(prefix):], data)
            return out
        }
    }
    return []byte("error: service not found")
}
