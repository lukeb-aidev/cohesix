// CLASSIFICATION: COMMUNITY
// Filename: context_test.go v0.1
// Author: Lukas Bower
// Date Modified: 2025-07-11

package agentsdk

import "testing"

func TestNew(t *testing.T) {
	ctx := New()
	if ctx == nil {
		t.Fatal("context should not be nil")
	}
}
