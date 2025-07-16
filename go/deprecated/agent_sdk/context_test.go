// CLASSIFICATION: COMMUNITY
// Filename: context_test.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-15

package agentsdk

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestNew(t *testing.T) {
	ctx := New()
	if ctx == nil {
		t.Fatal("context should not be nil")
	}
}

func TestCancellationPropagation(t *testing.T) {
	ac := New()
	c, cancel := context.WithCancel(context.Background())
	go func() {
		time.Sleep(10 * time.Millisecond)
		cancel()
	}()
	err := ac.Run(c, func(ctx context.Context) error {
		<-ctx.Done()
		return ctx.Err()
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("expected cancel error, got %v", err)
	}
}

func TestTimeoutExpiry(t *testing.T) {
	ac := New()
	c, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	err := ac.Run(c, func(ctx context.Context) error {
		<-ctx.Done()
		return ctx.Err()
	})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected deadline exceeded, got %v", err)
	}
}

func TestFaultInjection(t *testing.T) {
	ac := New()
	err := ac.Run(context.Background(), func(ctx context.Context) error {
		panic("boom")
	})
	if err == nil || err.Error() == "" {
		t.Fatal("expected panic error")
	}

	if err := ac.Run(context.Background(), func(ctx context.Context) error { return nil }); err != nil {
		t.Fatalf("restart failed: %v", err)
	}

	c, cancel := context.WithTimeout(context.Background(), 10*time.Millisecond)
	defer cancel()
	err = ac.Run(c, func(ctx context.Context) error {
		select {
		case <-time.After(time.Second):
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("expected timeout, got %v", err)
	}
}
