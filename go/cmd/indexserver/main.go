// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.1
// Author: Lukas Bower
// Date Modified: 2027-01-31
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
    "os"
    "path/filepath"
    "strings"
    "sync"
    "time"
)

var index = make(map[string][]string)
var mu sync.RWMutex

func buildIndex(root string) {
    filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
        if err != nil {
            return nil
        }
        name := d.Name()
        mu.Lock()
        index[name] = append(index[name], path)
        mu.Unlock()
        return nil
    })
}

func search(q string) []string {
    mu.RLock()
    defer mu.RUnlock()
    return index[q]
}

func main() {
    os.MkdirAll("/srv/index", 0755)
    os.WriteFile("/srv/index/query", []byte{}, 0644)
    os.WriteFile("/srv/index/results", []byte{}, 0644)
    buildIndex("/")
    last := ""
    for {
        data, _ := os.ReadFile("/srv/index/query")
        q := strings.TrimSpace(string(data))
        if q != "" && q != last {
            res := search(q)
            os.WriteFile("/srv/index/results", []byte(strings.Join(res, "\n")), 0644)
            last = q
        }
        time.Sleep(1 * time.Second)
    }
}
