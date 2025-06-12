// CLASSIFICATION: COMMUNITY
// Filename: flags_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22
package main

import (
    "flag"
    "testing"
)

func TestDefaultListenFlag(t *testing.T) {
    fs := flag.NewFlagSet("test", flag.ContinueOnError)
    listen := fs.String("listen", ":5640", "")
    if err := fs.Parse([]string{}); err != nil {
        t.Fatal(err)
    }
    if *listen != ":5640" {
        t.Fatalf("expected default :5640 got %s", *listen)
    }
}
