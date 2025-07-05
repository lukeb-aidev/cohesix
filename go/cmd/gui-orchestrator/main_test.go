// CLASSIFICATION: COMMUNITY
// Filename: main_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-22
package main

import (
	"flag"
	"io/ioutil"
	"path/filepath"
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

func TestLoadCreds(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "creds.json")
	data := []byte(`{"user":"u","pass":"p"}`)
	if err := ioutil.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("write: %v", err)
	}
	u, p, err := loadCreds(path)
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if u != "u" || p != "p" {
		t.Fatalf("unexpected values: %s %s", u, p)
	}
}
