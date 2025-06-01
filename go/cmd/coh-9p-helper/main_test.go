// CLASSIFICATION: COMMUNITY
// Filename: main_test.go
// Author: Panel Script
// Date Modified: 2025-06-01
package main

import (
"net"
"testing"
"time"
)

func TestListenerAccepts(t *testing.T) {
go main()
time.Sleep(100 * time.Millisecond) // give listener time
conn, err := net.Dial("tcp", ":5640")
if err != nil {
t.Fatalf("dial: %v", err)
}
_ = conn.Close()
}
