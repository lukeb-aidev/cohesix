// CLASSIFICATION: COMMUNITY
// Filename: mux_test.go v0.1
// Date Modified: 2025-07-07
// Author: Lukas Bower
package p9

import "testing"

type dummy struct{}

func (d dummy) Handle(path string, data []byte) ([]byte, error) { return data, nil }

func TestMuxRegister(t *testing.T) {
    m := NewMux()
    m.Register("/srv/test", dummy{})
    out := m.Handle("/srv/test/foo", []byte("hi"))
    if string(out) != "hi" {
        t.Fatalf("expected echo")
    }
}
