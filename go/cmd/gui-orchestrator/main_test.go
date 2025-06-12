// CLASSIFICATION: COMMUNITY
// Filename: main_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22
package main

import (
    "flag"
    "testing"
)

func TestPortFlagDefault(t *testing.T) {
    fs := flag.NewFlagSet("test", flag.ContinueOnError)
    port := fs.Int("port", 8888, "listen port")
    if err := fs.Parse([]string{}); err != nil {
        t.Fatalf("parse: %v", err)
    }
    if *port != 8888 {
        t.Fatalf("expected default 8888, got %d", *port)
    }
}
