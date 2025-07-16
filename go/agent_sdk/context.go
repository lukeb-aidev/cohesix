// CLASSIFICATION: COMMUNITY
// Filename: context.go v0.2
// Author: Lukas Bower
// Date Modified: 2025-07-15

package agentsdk

import (
	"context"
	"encoding/json"
	"fmt"
	"log"
	"os"
	"sync"
	"time"
)

type AgentContext struct {
	mu            sync.RWMutex
	Role          string
	Uptime        string
	LastGoal      map[string]any
	WorldSnapshot map[string]any
	TraceID       string
}

func readFile(path string) []byte {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	return data
}

func logEvent(role, event, traceID string) {
	rec := map[string]any{
		"ts":       time.Now().Format(time.RFC3339Nano),
		"role":     role,
		"event":    event,
		"trace_id": traceID,
	}
	b, _ := json.Marshal(rec)
	log.Print(string(b))
}

// UpdateLastGoal safely updates the agent's last goal.
func (a *AgentContext) UpdateLastGoal(goal map[string]any) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.LastGoal = goal
}

// LastGoalCopy returns a snapshot of the last goal.
func (a *AgentContext) LastGoalCopy() map[string]any {
	a.mu.RLock()
	defer a.mu.RUnlock()
	cp := make(map[string]any, len(a.LastGoal))
	for k, v := range a.LastGoal {
		cp[k] = v
	}
	return cp
}

// WorldSnapshotCopy returns a snapshot of the world state.
func (a *AgentContext) WorldSnapshotCopy() map[string]any {
	a.mu.RLock()
	defer a.mu.RUnlock()
	cp := make(map[string]any, len(a.WorldSnapshot))
	for k, v := range a.WorldSnapshot {
		cp[k] = v
	}
	return cp
}

func New() *AgentContext {
	ctx := &AgentContext{
		Role:          "Unknown",
		Uptime:        "0",
		LastGoal:      map[string]any{},
		WorldSnapshot: map[string]any{},
	}
	if b := readFile("/srv/agent_meta/role.txt"); b != nil {
		ctx.Role = string(b)
	}
	if b := readFile("/srv/agent_meta/uptime.txt"); b != nil {
		ctx.Uptime = string(b)
	}
	if b := readFile("/srv/agent_meta/trace_id.txt"); b != nil {
		ctx.TraceID = string(b)
	}
	if b := readFile("/srv/agent_meta/last_goal.json"); b != nil {
		json.Unmarshal(b, &ctx.LastGoal)
	}
	if b := readFile("/srv/world_state/world.json"); b != nil {
		json.Unmarshal(b, &ctx.WorldSnapshot)
	}
	logEvent(ctx.Role, "agent_init", ctx.TraceID)
	return ctx
}

type traceIDKey struct{}

// Run executes fn within the provided context and attaches the trace ID.
// It logs lifecycle events for debugging and recovers from panics.
func (a *AgentContext) Run(ctx context.Context, fn func(context.Context) error) (err error) {
	if ctx == nil {
		ctx = context.Background()
	}
	ctx = context.WithValue(ctx, traceIDKey{}, a.TraceID)
	logEvent(a.Role, "agent_start", a.TraceID)
	log.Printf("[role=%s trace=%s] run begin", a.Role, a.TraceID)

	done := make(chan struct{})
	go func() {
		defer func() {
			if r := recover(); r != nil {
				err = fmt.Errorf("panic: %v", r)
			}
			close(done)
		}()
		if e := fn(ctx); e != nil {
			err = e
		}
	}()

	select {
	case <-ctx.Done():
		log.Printf("[role=%s trace=%s] context done: %v", a.Role, a.TraceID, ctx.Err())
		return ctx.Err()
	case <-done:
		log.Printf("[role=%s trace=%s] run end", a.Role, a.TraceID)
		return err
	}
}

// Shutdown emits shutdown event for graceful teardown.
func (a *AgentContext) Shutdown(ctx context.Context) error {
	if ctx == nil {
		ctx = context.Background()
	}
	ctx = context.WithValue(ctx, traceIDKey{}, a.TraceID)
	logEvent(a.Role, "agent_shutdown", a.TraceID)
	log.Printf("[role=%s trace=%s] shutdown", a.Role, a.TraceID)
	return nil
}
