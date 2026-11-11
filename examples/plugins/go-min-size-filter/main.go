// Skip videos smaller than a size, such as samples, trailers and extras. The Go port of
// examples/plugins/min-size-filter, plus a count of finished downloads in the store.
package main

import "strings"

type settings struct {
	MinMB      int64    `json:"min_mb"`
	Extensions []string `json:"extensions"`
}

type hookFile struct {
	Name      string `json:"name"`
	Size      int64  `json:"size"`
	Extension string `json:"extension"`
	Decision  string `json:"decision"`
}

var (
	minBytes int64 = 50 << 20
	videos         = map[string]bool{}
)

//go:wasmexport init
func initPlugin(ptr, size uint32) uint64 {
	take(ptr, size)
	var cfg settings
	if msg := call("config", &cfg); msg != "" {
		fail(msg)
		return 0
	}
	if cfg.MinMB > 0 {
		minBytes = cfg.MinMB << 20
	}
	for _, ext := range cfg.Extensions {
		videos[strings.ToLower(ext)] = true
	}
	return 0
}

//go:wasmexport filter
func filter(ptr, size uint32) uint64 {
	var file hookFile
	if !args(ptr, size, &file) {
		fail("bad file")
		return 0
	}
	if file.Decision != "download" && file.Decision != "stream" {
		return result(nil)
	}
	if videos[file.Extension] && file.Size < minBytes {
		return result("skip")
	}
	return result(nil)
}

//go:wasmexport on_event
func onEvent(ptr, size uint32) uint64 {
	var event struct {
		Type string `json:"type"`
	}
	if !args(ptr, size, &event) {
		fail("bad event")
		return 0
	}
	var seen float64
	call("store.get", &seen, "downloads")
	if event.Type == "download.finished" {
		seen++
		if msg := call("store.set", nil, "downloads", seen); msg != "" {
			fail(msg)
			return 0
		}
	}
	logInfo("event", map[string]any{"type": event.Type, "downloads": seen})
	return result(nil)
}
