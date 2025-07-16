// CLASSIFICATION: COMMUNITY
// Filename: cli.go v0.2
// Date Modified: 2025-06-01
// Author: Lukas Bower
//
// ─────────────────────────────────────────────────────────────
// Cohesix · Go CLI Scaffold
//
// Provides a *minimal* Cobra root command that other Cohesix Go
// tools can embed (`go run ./go/cmd/...`).  The goal is to keep
// coupling low while giving developers a quick way to add sub‑
// commands (e.g. `snapshot`, `pkg`, `net`, etc.).
//
// Downstream binaries should call `tooling.Execute()` from their
// `main()`.
//
// Example:
//
//   package main
//
//   import "cohesix/internal/tooling"
//
//   func main() { tooling.Execute() }
// ─────────────────────────────────────────────────────────────
package tooling

import (
	"fmt"
	"os"

	"github.com/spf13/cobra"
)

// rootCmd is exported so other packages can add sub‑commands in init().
var rootCmd = &cobra.Command{
	Use:   "coh",
	Short: "Cohesix developer CLI",
	Long: `Cohesix multi‑purpose developer CLI.

Run "coh --help" to see available commands. Sub‑commands may be
added by separate Go packages at init() time.`,
}

// Execute runs the CLI.  Typically called from main().
func Execute() {
	if err := rootCmd.Execute(); err != nil {
		fmt.Fprintf(os.Stderr, "error: %v\n", err)
		os.Exit(1)
	}
}

func init() {
	// Built‑in `version` sub‑command.
	rootCmd.AddCommand(&cobra.Command{
		Use:   "version",
		Short: "Print Cohesix CLI version",
		Run: func(cmd *cobra.Command, args []string) {
			fmt.Println("Cohesix CLI v0.1.0 (stub)")
		},
	})
}