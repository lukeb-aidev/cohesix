// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.1
// Author: Lukas Bower
// Date Modified: 2027-01-31
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
    "bufio"
    "fmt"
    "os"
    "strings"
    "sync"
    "time"

    "github.com/fsnotify/fsnotify"
)

func main() {
    os.MkdirAll("/dev/watch", 0755)
    os.WriteFile("/dev/watch/ctl", []byte{}, 0644)
    f, _ := os.OpenFile("/dev/watch/events", os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0644)
    f.Close()

    watcher, _ := fsnotify.NewWatcher()
    var mu sync.Mutex
    watched := make(map[string]bool)

    go func() {
        out, _ := os.OpenFile("/dev/watch/events", os.O_WRONLY|os.O_APPEND, 0644)
        defer out.Close()
        for {
            select {
            case ev := <-watcher.Events:
                fmt.Fprintf(out, "%s %s\n", ev.Name, ev.Op.String())
            case err := <-watcher.Errors:
                fmt.Fprintf(out, "error %v\n", err)
            }
        }
    }()

    for {
        data, _ := os.ReadFile("/dev/watch/ctl")
        scanner := bufio.NewScanner(strings.NewReader(string(data)))
        for scanner.Scan() {
            p := strings.TrimSpace(scanner.Text())
            if p == "" {
                continue
            }
            mu.Lock()
            if !watched[p] {
                watcher.Add(p)
                watched[p] = true
            }
            mu.Unlock()
        }
        time.Sleep(1 * time.Second)
    }
}
