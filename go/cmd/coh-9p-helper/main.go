// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.2
// Date Modified: 2025-06-01
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
//   go run ./go/cmd/coh-9p-helper --listen :5640
// ─────────────────────────────────────────────────────────────
package main

import (
	"flag"
	"fmt"
	"io"
	"log"
	"net"
)

var listenAddr = flag.String("listen", ":5640", "TCP address to listen on")

func handleConn(c net.Conn) {
	defer c.Close()
	buf := make([]byte, 4096)

	for {
		n, err := c.Read(buf)
		if err != nil {
			if err != io.EOF {
				log.Printf("read error: %v", err)
			}
			return
		}
		log.Printf("received %d‑byte 9P message (stub)", n)

		// TODO: forward to Unix socket / parse 9P
	}
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
