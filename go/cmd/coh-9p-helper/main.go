// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.4
// Date Modified: 2026-07-27
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix 9P‑helper · Minimal TCP Proxy (stub)
//
// Listens on a configurable TCP port and simply dumps the length
// of incoming 9P packets to stdout.  The real implementation will
// forward between a Unix‑domain socket and a remote Worker, but
// this stub is enough for smoke tests and Go vet/CI.
//
// Usage:
//
//	go run ./go/cmd/coh-9p-helper --listen :5640 [--socket /path/to.sock]
//
// The socket path defaults to filepath.Join(os.TempDir(), "coh9p.sock") or the
// value of the COH9P_SOCKET environment variable.
//
// ─────────────────────────────────────────────────────────────
package main

import (
	"flag"
	"io"
	"log"
	"net"
	"os"
	"path/filepath"
)

var listenAddr = flag.String("listen", ":5640", "TCP address to listen on")
var socketPath = flag.String("socket", "", "Unix socket path for 9P server")
var unixSocket string

func handleConn(c net.Conn) {
	defer c.Close()
	u, err := net.Dial("unix", unixSocket)
	if err != nil {
		log.Printf("unix dial error: %v", err)
		return
	}
	defer u.Close()

	go io.Copy(u, c)
	io.Copy(c, u)
}

func main() {
	flag.Parse()

	unixSocket = *socketPath
	if unixSocket == "" {
		unixSocket = os.Getenv("COH9P_SOCKET")
		if unixSocket == "" {
			unixSocket = filepath.Join(os.TempDir(), "coh9p.sock")
		}
	}

	l, err := net.Listen("tcp", *listenAddr)
	if err != nil {
		log.Fatal("listen:", err)
	}
	log.Printf("coh-9p-helper listening on %s", *listenAddr)

	for {
		conn, err := l.Accept()
		if err != nil {
			log.Printf("accept error: %v", err)
			continue
		}
		go handleConn(conn)
	}
}
