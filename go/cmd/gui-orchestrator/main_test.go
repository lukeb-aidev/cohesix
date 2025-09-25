// CLASSIFICATION: COMMUNITY
// Filename: main_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2029-02-21
package main

import (
	"flag"
	"os"
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
	data := []byte(`{"user":"u","pass":"p","roles":["QueenPrimary"],"tls_cert":"cert.pem","tls_key":"key.pem","client_ca":"ca.pem"}`)
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("write: %v", err)
	}
	creds, err := loadCreds(path)
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if creds.User != "u" || creds.Pass != "p" {
		t.Fatalf("unexpected values: %s %s", creds.User, creds.Pass)
	}
	if len(creds.Roles) != 1 || creds.Roles[0] != "QueenPrimary" {
		t.Fatalf("unexpected roles: %v", creds.Roles)
	}
	if creds.TLSCert != "cert.pem" || creds.TLSKey != "key.pem" || creds.ClientCA != "ca.pem" {
		t.Fatalf("unexpected tls settings: %+v", creds)
	}
}
