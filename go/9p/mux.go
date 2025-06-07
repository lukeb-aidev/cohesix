// CLASSIFICATION: COMMUNITY
// Filename: mux.go v0.1
// Date Modified: 2025-06-07
// Author: Lukas Bower
//
// Simple 9P multiplexer helper used by integration tests. It waits on
// multiple service channels and forwards requests to the Cohesix runtime.
package p9

import (
        "context"
)

type Request struct {
        Path string
        Data []byte
}

type ServiceChan <-chan Request

type Mux struct {
        services map[string]ServiceChan
}

func NewMux() *Mux {
        return &Mux{services: make(map[string]ServiceChan)}
}

func (m *Mux) Register(name string, ch ServiceChan) {
        m.services[name] = ch
}

func (m *Mux) Serve(ctx context.Context) {
        for {
                select {
                case <-ctx.Done():
                        return
                default:
                        for name, ch := range m.services {
                                select {
                                case req := <-ch:
                                        _ = req
                                        // In a real implementation this would translate to 9P calls.
                                default:
                                }
                                _ = name
                        }
                }
        }
}


// Package p9 provides a simple concurrent request multiplexer that
// mirrors the behaviour of the Rust counterpart.  Each service is
// identified by name and implements the Handler interface.
package p9

import (
	"sync"
)

// Handler represents a service capable of handling a 9P path.
type Handler interface {
	Handle(path string, data []byte) ([]byte, error)
}

// Mux routes requests to registered handlers.
type Mux struct {
	mu       sync.RWMutex
	services map[string]Handler
}

// NewMux returns a ready-to-use multiplexer.
func NewMux() *Mux {
	return &Mux{services: make(map[string]Handler)}
}

// Register adds a named service.
func (m *Mux) Register(name string, h Handler) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.services[name] = h
}

// Handle routes the request in a goroutine and returns a channel with the response.
func (m *Mux) Handle(path string, data []byte) <-chan []byte {
	ch := make(chan []byte, 1)
	go func() {
		m.mu.RLock()
		var h Handler
		for prefix, handler := range m.services {
			if len(path) >= len(prefix) && path[:len(prefix)] == prefix {
				h = handler
				path = path[len(prefix):]
				break
			}
		}
		m.mu.RUnlock()
		if h == nil {
			ch <- []byte("error: service not found")
			return
		}
		resp, _ := h.Handle(path, data)
		ch <- resp
	}()
	return ch
}
