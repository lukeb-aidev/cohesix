// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.3
// Date Modified: 2025-06-10
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
//	go run ./go/cmd/coh-9p-helper --listen :5640
//
// ─────────────────────────────────────────────────────────────
package main

import (
	"flag"
	"io"
	"log"
	"net"
)

var listenAddr = flag.String("listen", ":5640", "TCP address to listen on")

func handleConn(c net.Conn) {
	defer c.Close()
	u, err := net.Dial("unix", "/tmp/coh9p.sock")
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
