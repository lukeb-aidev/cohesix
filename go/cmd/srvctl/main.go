// CLASSIFICATION: COMMUNITY
// Filename: main.go v0.1
// Author: Lukas Bower
// Date Modified: 2027-01-31
// License: SPDX-License-Identifier: MIT OR Apache-2.0

package main

import (
    "flag"
    "fmt"
    "os"
    "path/filepath"
)

func announce(args []string) {
    fs := flag.NewFlagSet("announce", flag.ExitOnError)
    name := fs.String("name", "", "service name")
    version := fs.String("version", "0", "service version")
    fs.Parse(args)
    rest := fs.Args()
    if *name == "" || len(rest) < 1 {
        fmt.Fprintln(os.Stderr, "usage: srvctl announce -name mysvc -version 1.0 /path")
        os.Exit(1)
    }
    srvDir := filepath.Join("/srv/services", *name)
    os.MkdirAll(srvDir, 0755)
    info := fmt.Sprintf("name=%s\nversion=%s\npath=%s\n", *name, *version, rest[0])
    os.WriteFile(filepath.Join(srvDir, "info"), []byte(info), 0644)
    os.WriteFile(filepath.Join(srvDir, "ctl"), []byte{}, 0644)
}

func main() {
    if len(os.Args) < 2 {
        fmt.Fprintln(os.Stderr, "usage: srvctl announce [args]")
        os.Exit(1)
    }
    cmd := os.Args[1]
    switch cmd {
    case "announce":
        announce(os.Args[2:])
    default:
        fmt.Fprintln(os.Stderr, "unknown command")
        os.Exit(1)
    }
}
