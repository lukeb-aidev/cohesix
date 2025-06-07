// CLASSIFICATION: COMMUNITY
// Filename: mux.go v0.1
// Date Modified: 2025-06-18
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

